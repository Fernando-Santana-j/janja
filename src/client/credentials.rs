//! Passwords live only in the OS credential store, never in the profile or logs.
//! Calls block: invoke from the dedicated vault worker, outside GTK and media workers.
//! Linux requires an existing, unlocked default Secret Service collection.

const MAX_PASSWORD: usize = 128;

pub(super) fn load(room: u64, name: &str) -> Result<Option<String>, String> {
    platform::load(&key(room, name)?)
}

pub(super) fn save(room: u64, name: &str, password: &str) -> Result<(), String> {
    let key = key(room, name)?;
    validate_password(password)?;
    platform::save(&key, password)
}

pub(super) fn remove(room: u64, name: &str) -> Result<(), String> {
    platform::remove(&key(room, name)?)
}

fn key(room: u64, name: &str) -> Result<String, String> {
    if room == 0
        || name.trim().is_empty()
        || name.chars().count() > 32
        || name.chars().any(char::is_control)
    {
        return Err("Sala inválida para guardar a senha.".into());
    }
    Ok(namespace(super::ENDPOINT, room, name))
}

fn namespace(endpoint: &str, room: u64, name: &str) -> String {
    use ring::digest::{Context, SHA256};
    use std::fmt::Write;
    let mut hash = Context::new(&SHA256);
    // Lengths separate fields unambiguously; room IDs can be reused with another name.
    hash.update(&(endpoint.len() as u64).to_be_bytes());
    hash.update(endpoint.as_bytes());
    hash.update(&room.to_be_bytes());
    hash.update(&(name.len() as u64).to_be_bytes());
    hash.update(name.as_bytes());
    let mut key = String::from("Janja/room/v1/");
    for byte in hash.finish().as_ref() {
        let _ = write!(key, "{byte:02x}");
    }
    key
}

fn validate_password(password: &str) -> Result<(), String> {
    if !(8..=MAX_PASSWORD).contains(&password.len()) {
        return Err("A senha guardada deve ter 8–128 bytes.".into());
    }
    Ok(())
}

fn decode(bytes: Vec<u8>) -> Result<String, String> {
    let password = String::from_utf8(bytes)
        .map_err(|_| "O cofre devolveu uma senha em formato inválido.".to_string())?;
    validate_password(&password)?;
    Ok(password)
}

#[cfg(target_os = "linux")]
mod platform {
    use dbus::{
        Path,
        arg::{PropMap, Variant},
        blocking::{Connection, Proxy, stdintf::org_freedesktop_dbus::Properties},
    };
    use std::{collections::HashMap, time::Duration};

    const SERVICE: &str = "org.freedesktop.secrets";
    const ROOT: &str = "/org/freedesktop/secrets";
    const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
    const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
    const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
    const LOCKED: &str = "Desbloqueie o cofre de senhas do sistema e tente novamente.";
    type Secret = (Path<'static>, Vec<u8>, Vec<u8>, String);

    // Each private bus connection owns its sessions; dropping it closes them on every exit.
    struct Store(Connection);
    impl Store {
        fn open() -> Result<Self, String> {
            Connection::new_session().map(Self).map_err(unavailable)
        }

        fn proxy<'a>(&'a self, path: &'a str) -> Proxy<'a, &'a Connection> {
            self.0.with_proxy(SERVICE, path, Duration::from_secs(2))
        }

        fn find(&self, key: &str) -> Result<Vec<Path<'static>>, String> {
            let (unlocked, locked): (Vec<Path<'static>>, Vec<Path<'static>>) = self
                .proxy(ROOT)
                .method_call(SERVICE_IFACE, "SearchItems", (attributes(key),))
                .map_err(unavailable)?;
            if !locked.is_empty() {
                return Err(LOCKED.into());
            }
            if unlocked.len() > 1 {
                return Err("Há senhas duplicadas para esta sala no cofre do sistema.".into());
            }
            Ok(unlocked)
        }

        fn session(&self) -> Result<Path<'static>, String> {
            // plain uses the authenticated local bus; persistence belongs to the system keyring.
            let (output, path): (Variant<String>, Path<'static>) = self
                .proxy(ROOT)
                .method_call(SERVICE_IFACE, "OpenSession", ("plain", Variant("")))
                .map_err(unavailable)?;
            if path == Path::from("/") || !output.0.is_empty() {
                return Err("O cofre não aceitou a sessão de senhas.".into());
            }
            Ok(path)
        }
    }

    fn unavailable(_: dbus::Error) -> String {
        // Do not forward service error bodies: they are outside our control and may contain data.
        "Cofre de senhas indisponível. Confira o Secret Service e desbloqueie o cofre do sistema."
            .into()
    }

    fn attributes(key: &str) -> HashMap<String, String> {
        HashMap::from([
            ("application".into(), "br.com.infocoretech.Janja".into()),
            ("room-key".into(), key.into()),
        ])
    }

    fn no_prompt(prompt: &Path<'_>) -> Result<(), String> {
        if prompt == &Path::from("/") {
            Ok(())
        } else {
            // Never call Prompt or Unlock: an unattended operation must not open a dialog.
            Err(LOCKED.into())
        }
    }

    pub(super) fn load(key: &str) -> Result<Option<String>, String> {
        let store = Store::open()?;
        let items = store.find(key)?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        let session = store.session()?;
        let (secret,): (Secret,) = store
            .proxy(item)
            .method_call(ITEM_IFACE, "GetSecret", (&session,))
            .map_err(unavailable)?;
        if secret.0 != session || !secret.1.is_empty() {
            return Err("O cofre devolveu uma sessão de senhas inválida.".into());
        }
        super::decode(secret.2).map(Some)
    }

    pub(super) fn save(key: &str, password: &str) -> Result<(), String> {
        let store = Store::open()?;
        let (collection,): (Path<'static>,) = store
            .proxy(ROOT)
            .method_call(SERVICE_IFACE, "ReadAlias", ("default",))
            .map_err(unavailable)?;
        if collection == Path::from("/") {
            return Err(
                "Crie e desbloqueie um cofre padrão no gerenciador de senhas do sistema.".into(),
            );
        }
        let proxy = store.proxy(&collection);
        let locked: bool = proxy.get(COLLECTION_IFACE, "Locked").map_err(unavailable)?;
        if locked {
            return Err(LOCKED.into());
        }
        let mut properties = PropMap::new();
        properties.insert(
            format!("{ITEM_IFACE}.Label"),
            Variant(Box::new("Janja — senha de sala".to_string())),
        );
        properties.insert(
            format!("{ITEM_IFACE}.Attributes"),
            Variant(Box::new(attributes(key))),
        );
        let secret: Secret = (
            store.session()?,
            Vec::new(),
            password.as_bytes().to_vec(),
            "text/plain; charset=utf-8".into(),
        );
        let (item, prompt): (Path<'static>, Path<'static>) = proxy
            .method_call(COLLECTION_IFACE, "CreateItem", (properties, secret, true))
            .map_err(unavailable)?;
        no_prompt(&prompt)?;
        if item == Path::from("/") {
            return Err("O cofre não confirmou a gravação da senha.".into());
        }
        Ok(())
    }

    pub(super) fn remove(key: &str) -> Result<(), String> {
        let store = Store::open()?;
        for item in store.find(key)? {
            let (prompt,): (Path<'static>,) = store
                .proxy(&item)
                .method_call(ITEM_IFACE, "Delete", ())
                .map_err(unavailable)?;
            no_prompt(&prompt)?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn secret_tuple_roundtrips_without_connecting_to_a_bus() {
            let secret: Secret = (
                Path::from("/session/test"),
                Vec::new(),
                "senha-sintética".as_bytes().to_vec(),
                "text/plain; charset=utf-8".into(),
            );
            let message = dbus::Message::new_method_call(SERVICE, ROOT, ITEM_IFACE, "GetSecret")
                .unwrap()
                .append1(secret.clone());
            assert_eq!(message.read1::<Secret>().unwrap(), secret);
            assert!(no_prompt(&Path::from("/")).is_ok());
            assert!(no_prompt(&Path::from("/prompt/test")).is_err());
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{ffi::c_void, ptr};

    const GENERIC: u32 = 1;
    const LOCAL_MACHINE: u32 = 2;
    const NOT_FOUND: i32 = 1168;

    #[repr(C)]
    struct Credential {
        flags: u32,
        kind: u32,
        target: *mut u16,
        comment: *mut u16,
        last_written: [u32; 2], // FILETIME has 4-byte alignment, including on 32-bit Windows.
        blob_size: u32,
        blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut c_void,
        target_alias: *mut u16,
        username: *mut u16,
    }

    const _: () = assert!(
        std::mem::size_of::<Credential>()
            == if cfg!(target_pointer_width = "64") {
                80
            } else {
                52
            }
    );

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn CredReadW(target: *const u16, kind: u32, flags: u32, out: *mut *mut Credential) -> i32;
        fn CredWriteW(credential: *const Credential, flags: u32) -> i32;
        fn CredDeleteW(target: *const u16, kind: u32, flags: u32) -> i32;
        fn CredFree(buffer: *mut c_void);
    }

    struct OwnedCredential(std::ptr::NonNull<Credential>);
    impl Drop for OwnedCredential {
        fn drop(&mut self) {
            // SAFETY: this is the unique non-null buffer returned by CredReadW, freed once.
            unsafe { CredFree(self.0.as_ptr().cast()) };
        }
    }

    fn wide(key: &str) -> Vec<u16> {
        key.encode_utf16().chain(Some(0)).collect()
    }

    fn unavailable(error: std::io::Error) -> String {
        format!(
            "Cofre de senhas do Windows indisponível (código {}). Confira a sessão do usuário.",
            error.raw_os_error().unwrap_or_default()
        )
    }

    pub(super) fn load(key: &str) -> Result<Option<String>, String> {
        let target = wide(key);
        let mut raw = ptr::null_mut();
        // SAFETY: target is NUL-terminated and live; raw is a writable output pointer.
        if unsafe { CredReadW(target.as_ptr(), GENERIC, 0, &mut raw) } == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(NOT_FOUND) {
                Ok(None)
            } else {
                Err(unavailable(error))
            };
        }
        let owned = OwnedCredential(
            std::ptr::NonNull::new(raw)
                .ok_or("O cofre do Windows devolveu uma credencial inválida.")?,
        );
        // SAFETY: CredReadW succeeded; OwnedCredential keeps its initialized buffer alive.
        let credential = unsafe { owned.0.as_ref() };
        if !(8..=super::MAX_PASSWORD as u32).contains(&credential.blob_size)
            || credential.blob.is_null()
        {
            return Err("O cofre do Windows devolveu uma senha em formato inválido.".into());
        }
        // SAFETY: the system returned blob_size readable bytes inside the still-owned buffer.
        let bytes =
            unsafe { std::slice::from_raw_parts(credential.blob, credential.blob_size as usize) };
        super::decode(bytes.to_vec()).map(Some)
    }

    pub(super) fn save(key: &str, password: &str) -> Result<(), String> {
        let mut target = wide(key);
        let mut username = wide("Janja");
        let credential = Credential {
            flags: 0,
            kind: GENERIC,
            target: target.as_mut_ptr(),
            comment: ptr::null_mut(),
            last_written: [0; 2],
            blob_size: password.len() as u32,
            blob: password.as_ptr().cast_mut(),
            persist: LOCAL_MACHINE,
            attribute_count: 0,
            attributes: ptr::null_mut(),
            target_alias: ptr::null_mut(),
            username: username.as_mut_ptr(),
        };
        // SAFETY: CREDENTIALW layout is repr(C); all input buffers live through this read-only call.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(unavailable(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    pub(super) fn remove(key: &str) -> Result<(), String> {
        let target = wide(key);
        // SAFETY: target is NUL-terminated and lives through this synchronous call.
        if unsafe { CredDeleteW(target.as_ptr(), GENERIC, 0) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(NOT_FOUND) {
                return Err(unavailable(error));
            }
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    pub(super) fn load(_: &str) -> Result<Option<String>, String> {
        Err("Cofre de senhas disponível apenas no Linux e Windows.".into())
    }
    pub(super) fn save(_: &str, _: &str) -> Result<(), String> {
        load("").map(|_| ())
    }
    pub(super) fn remove(_: &str) -> Result<(), String> {
        load("").map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_binds_endpoint_room_and_exact_name() {
        let key = namespace("wss://example.test/ws", 2, "Amigos");
        assert_eq!(key, namespace("wss://example.test/ws", 2, "Amigos"));
        assert_ne!(key, namespace("wss://other.test/ws", 2, "Amigos"));
        assert_ne!(key, namespace("wss://example.test/ws", 3, "Amigos"));
        assert_ne!(key, namespace("wss://example.test/ws", 2, "amigos"));
        assert_ne!(key, namespace("wss://example.test/ws", 2, "Amigos "));
        let hash = key.strip_prefix("Janja/room/v1/").unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn invalid_inputs_fail_before_accessing_the_real_store() {
        assert!(load(0, "Sala").is_err());
        assert!(save(2, " ", "12345678").is_err());
        assert!(save(2, "Sala", "short").is_err());
        assert!(save(2, "Sala", &"x".repeat(129)).is_err());
        assert!(remove(2, "Sala\n").is_err());
        assert!(key(2, &"á".repeat(32)).is_ok());
        assert!(key(2, &"á".repeat(33)).is_err());
    }

    #[test]
    fn stored_passwords_are_validated_without_exposing_their_content() {
        assert_eq!(decode("senha-áé".as_bytes().to_vec()).unwrap(), "senha-áé");
        assert!(decode(vec![0xff; 8]).is_err());
        assert!(decode(vec![b'x'; 129]).is_err());
        assert!(decode(Vec::new()).is_err());
    }
}
