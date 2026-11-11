use glib_old::translate::*;
use gst::prelude::*;
use serde_json::{Value, json};
use std::{
    ffi::CStr,
    os::raw::{c_char, c_int},
    ptr,
};

#[repr(C)]
struct Description {
    kind: c_int,
    sdp: *mut std::ffi::c_void,
}

pub fn ice_urls(servers: &Value) -> Result<Vec<String>, String> {
    if !crate::server::valid_ice(servers) {
        return Err("Configuração STUN/TURN inválida".into());
    }
    let encode = |s: &str| {
        url::form_urlencoded::byte_serialize(s.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    let mut urls = Vec::new();
    for server in servers.as_array().unwrap() {
        let values = match &server["urls"] {
            Value::String(s) => vec![s.as_str()],
            Value::Array(values) => values.iter().map(|v| v.as_str().unwrap()).collect(),
            _ => unreachable!(),
        };
        for value in values {
            let (scheme, host) = value.split_once(':').unwrap();
            let host = host.strip_prefix("//").unwrap_or(host);
            let plain = format!("{scheme}://{host}");
            let parsed = url::Url::parse(&plain).map_err(|_| "Endereço STUN/TURN inválido")?;
            if parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.fragment().is_some()
                || !parsed.path().is_empty()
            {
                return Err("Endereço STUN/TURN inválido".into());
            }
            urls.push(match scheme {
                "stun" => plain,
                "turn" | "turns" => format!(
                    "{scheme}://{}:{}@{host}",
                    encode(server["username"].as_str().unwrap()),
                    encode(server["credential"].as_str().unwrap())
                ),
                _ => return Err("Use STUN UDP ou TURN/TLS; STUN/TLS não é suportado".into()),
            });
        }
    }
    Ok(urls)
}

pub fn configure_ice(rtc: &gst::Element, urls: &[String]) -> Result<(), String> {
    let stun = urls.iter().find(|url| url.starts_with("stun://"));
    rtc.set_property("stun-server", &stun.map(String::as_str))
        .map_err(|e| e.to_string())?;
    for url in urls.iter().filter(|url| !url.starts_with("stun://")) {
        let accepted = rtc
            .emit("add-turn-server", &[&url.as_str()])
            .map_err(|_| "Não foi possível configurar TURN")?
            .and_then(|v| v.get::<bool>().ok().flatten());
        if accepted != Some(true) {
            return Err("O runtime rejeitou a configuração TURN".into());
        }
    }
    Ok(())
}
#[link(name = "gstsdp-1.0")]
unsafe extern "C" {
    fn gst_sdp_message_new(message: *mut *mut std::ffi::c_void) -> c_int;
    fn gst_sdp_message_parse_buffer(
        data: *const u8,
        size: u32,
        message: *mut std::ffi::c_void,
    ) -> c_int;
    fn gst_sdp_message_as_text(message: *const std::ffi::c_void) -> *mut c_char;
    fn gst_sdp_message_free(message: *mut std::ffi::c_void) -> c_int;
}
#[link(name = "gstwebrtc-1.0")]
unsafe extern "C" {
    fn gst_webrtc_session_description_new(
        kind: c_int,
        sdp: *mut std::ffi::c_void,
    ) -> *mut Description;
    fn gst_webrtc_session_description_free(desc: *mut Description);
}

pub fn create_description(
    rtc: &gst::Element,
    offer: bool,
    done: impl FnOnce(Result<Value, String>) + Send + 'static,
) -> Result<(), String> {
    super::diagnostics::event!("sdp-create", json!({"offer":offer}));
    let done = std::sync::Arc::new(std::sync::Mutex::new(Some(done)));
    let rtc_copy = rtc.clone();
    let field = if offer { "offer" } else { "answer" };
    let promise = gst::Promise::with_change_func(move |reply| {
        let result = (|| {
            let structure = reply
                .map_err(|e| format!("SDP: {e:?}"))?
                .ok_or("SDP sem resposta")?;
            let value = structure.get_value(field).map_err(|e| e.to_string())?;
            // The boxed description belongs to the promise and stays alive through this callback.
            unsafe {
                let desc =
                    gst::gobject_sys::g_value_get_boxed(value.to_glib_none().0) as *mut Description;
                if desc.is_null() {
                    return Err("SDP ausente".into());
                }
                let text = gst_sdp_message_as_text((*desc).sdp);
                if text.is_null() {
                    return Err("SDP inválido".into());
                }
                let sdp = CStr::from_ptr(text).to_string_lossy().into_owned();
                gst::glib_sys::g_free(text.cast());
                let completion = done.clone();
                let p = gst::Promise::with_change_func(move |reply| {
                    let result = reply
                        .map_err(|e| format!("SDP local: {e:?}"))
                        .and_then(|reply| {
                            if reply.is_some_and(|r| r.has_field("error")) {
                                Err("Não foi possível aplicar o SDP local".into())
                            } else {
                                Ok(json!({"description":{"type":field,"sdp":sdp}}))
                            }
                        });
                    super::diagnostics::event!(
                        "sdp-local-applied",
                        json!({"kind":field,"ok":result.is_ok()}),
                    );
                    if let Some(done) = completion.lock().unwrap().take() {
                        done(result);
                    }
                });
                gst::gobject_sys::g_signal_emit_by_name(
                    rtc_copy.as_ptr().cast(),
                    c"set-local-description".as_ptr(),
                    desc,
                    p.to_glib_none().0,
                );
                Ok(())
            }
        })();
        if let Err(error) = result
            && let Some(done) = done.lock().unwrap().take()
        {
            done(Err(error));
        }
    });
    rtc.emit(
        if offer {
            "create-offer"
        } else {
            "create-answer"
        },
        &[&None::<gst::Structure>, &promise],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_remote(
    rtc: &gst::Element,
    kind: &str,
    text: &str,
    done: impl FnOnce(Result<(), String>) + Send + 'static,
) -> Result<(), String> {
    super::diagnostics::event!("sdp-remote-request", json!({"kind":kind}));
    let kind = match kind {
        "offer" => 1,
        "answer" => 3,
        _ => return Err("Tipo SDP inválido".into()),
    };
    if text.len() > 60_000 || !text.starts_with("v=0") {
        return Err("SDP inválido".into());
    }
    unsafe {
        let mut sdp = ptr::null_mut();
        if gst_sdp_message_new(&mut sdp) != 0 {
            return Err("Falha ao alocar SDP".into());
        }
        if gst_sdp_message_parse_buffer(text.as_ptr(), text.len() as u32, sdp) != 0 {
            gst_sdp_message_free(sdp);
            return Err("SDP inválido".into());
        }
        let desc = gst_webrtc_session_description_new(kind, sdp);
        if desc.is_null() {
            gst_sdp_message_free(sdp);
            return Err("Falha no SDP".into());
        }
        let promise = gst::Promise::with_change_func(move |result| {
            super::diagnostics::event!(
                "sdp-remote-complete",
                json!({"kind":kind,"promise_ok":result.is_ok()}),
            );
            done(
                result
                    .map_err(|e| format!("SDP remoto: {e:?}"))
                    .and_then(|reply| {
                        if reply.is_some_and(|r| r.has_field("error")) {
                            Err("Não foi possível aplicar o SDP remoto".into())
                        } else {
                            Ok(())
                        }
                    }),
            );
        });
        gst::gobject_sys::g_signal_emit_by_name(
            rtc.as_ptr().cast(),
            c"set-remote-description".as_ptr(),
            desc,
            promise.to_glib_none().0,
        );
        gst_webrtc_session_description_free(desc);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ice_urls_escape_credentials_validate_hosts_and_configure_turn() {
        gst::init().unwrap();
        let urls = ice_urls(&json!([{
            "urls":["stun:127.0.0.1:3478", "turns:[::1]:5349?transport=tcp"],
            "username":"123:test @", "credential":"p:a+/=%"
        }]))
        .unwrap();
        assert_eq!(
            urls,
            [
                "stun://127.0.0.1:3478",
                "turns://123%3Atest%20%40:p%3Aa%2B%2F%3D%25@[::1]:5349?transport=tcp"
            ]
        );
        let rtc = gst::ElementFactory::make("webrtcbin", None).unwrap();
        configure_ice(&rtc, &urls).unwrap();
        configure_ice(&rtc, &[]).unwrap();
        assert!(
            rtc.get_property("stun-server")
                .unwrap()
                .get::<String>()
                .unwrap()
                .unwrap_or_default()
                .is_empty()
        );
        for bad in [
            json!([{"urls":"turn:example.org"}]),
            json!([{"urls":"stun:example.org/path"}]),
            json!([{"urls":"stun:user@example.org"}]),
            json!([{"urls":"stun:example.org#fragment"}]),
            json!([{"urls":"https://example.org"}]),
            json!([{"urls":[]}]),
        ] {
            assert!(ice_urls(&bad).is_err());
        }
    }
    #[test]
    fn rejects_invalid_sdp_before_ffi() {
        gst::init().unwrap();
        let rtc = gst::ElementFactory::make("webrtcbin", None).unwrap();
        assert!(set_remote(&rtc, "bogus", "v=0", |_| {}).is_err());
        assert!(set_remote(&rtc, "offer", "bad", |_| {}).is_err());
    }
}
