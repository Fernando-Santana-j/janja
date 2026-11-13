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
- [x] Concluir os testes adicionais da transação de atualização Windows e a análise do Defender.
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

A validação adicional confirmou aplicação real da atualização, reinstalação e
desinstalação no Windows. O Defender não detectou ameaças no portátil nem no
instalador; os hashes foram guardados em `dist/windows-security.json`. O Actions
foi bloqueado pela conta antes de compilar. A publicação local usa esses mesmos
pacotes e só libera o rascunho depois de verificar os arquivos enviados. O pacote
Linux local exige glibc 2.44; o build Ubuntu 24.04 do CI não foi executado.


Revisão final r13: o desenho bilinear com bordas limitadas à imagem mantém o
conteúdo inteiro e permite o caminho rápido opaco do Cairo. A medição alternada
na função real passou de 25,26 para 3,64 ms/quadro em 1080p → 1440p; com escala de
dispositivo 2×, de 24,90 para 3,44 ms/quadro. Esses números medem somente desenho,
não CPU total nem latência de transmissão. Os testes incluem faixas pretas e
bordas em escala 1×/2×. A mídia, codecs, filas e contadores necessários ao
watchdog foram preservados. Validação Windows da r13 e publicação ainda pendentes.


A validação Windows da r13 parou numa asserção de áudio. A bancada reproduziu
os mesmos números ao interromper a saída do jitterbuffer por 120 ms: dois
pacotes realmente ausentes exigiram 960 amostras de compensação, embora o contador
de perdas por prazo mostrasse apenas um. O teste agora observa as lacunas RTP
reais e verifica a compensação exata, incluindo descarte pela fila e volta do
contador de sequência. Três cenários passaram em vinte repetições; uma mutação
que duplica a compensação continua falhando. A confirmação do caso original e a
validação completa no Windows permanecem pendentes. O áudio de produção não foi
alterado por essa correção de teste.
