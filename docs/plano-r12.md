# Janja r12 — desempenho, tela cheia e distribuição

Objetivo: reduzir trabalho do cliente sem reintroduzir congelamentos ou falhas de áudio.

- [x] Medir renderização Cairo em redução e ampliação; comparar filtros nas mesmas imagens.
- [x] Mostrar a imagem inteira por padrão em tela cheia, preservando proporção e seleção automática do monitor.
- [x] Remover do cliente comum somente instrumentação comprovadamente dispensável; manter os testes de regressão fora dele.
- [x] Executar testes de mídia, análise estática e comparação com a linha de base Linux.
- [x] Verificar o receptor da biblioteca comum sem diagnóstico e provar que o teste detecta a remoção do contador necessário ao watchdog.
- [x] Preparar comparação automática com carga sintética de CPU e GPU, executada separadamente do cliente.
- [x] Implementar instaladores e lançadores de atualização com Releases assinadas no GitHub.
- [x] Concluir a validação Windows de mídia, carga sintética e instalação/reinstalação/desinstalação.
- [ ] Concluir os testes adicionais da transação de atualização Windows e a análise do Defender.
- [ ] Validar atualização real entre duas Releases antes da distribuição ampla.
- [ ] Medir a queda de FPS no jogo durante uma sessão real.

O teste Windows inicial revelou uma suposição incorreta do teste de
redimensionamento: ele selecionava um encoder de hardware sem suporte validado a
essa mudança ao vivo. Esse cenário agora usa software; os testes comuns de
transmissão continuam selecionando o hardware disponível. A produção mantém a
restrição de redimensionamento para backends ainda não validados.

Restrições: preservar o decoder Windows que passou nos testes, Opus/FEC, jitter adaptativo,
limites de filas, recuperação de quadros e inibição de descanso. Não trocar codecs ou
aumentar buffers para mascarar perdas de desempenho.

Atualizações: arquivos por HTTPS no GitHub escolhido pelo responsável, metadados
assinados e recuperação dos arquivos da versão anterior. Consultar somente ao abrir;
falha de atualização deve permitir abrir a versão atual. A chave privada permanece
fora do Git e dos pacotes. A publicação foi autorizada pelo responsável e deve
ocorrer após as verificações; preservar o servidor de sinalização. Detalhes
operacionais em `docs/atualizacoes.md`.
