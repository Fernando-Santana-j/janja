# Interface nativa Janja

Produto de uma tarefa: entrar em uma sala, compartilhar uma fonte e assistir.
Referência de interação: Discord. Tecnologia: GTK3, Cairo e fontes do sistema.

## Composição

- Cabeçalho persistente: identidade, perfil e estado real da conexão.
- Salas: busca local, nome, ocupação, proteção e ação de entrada; sala cheia
  tem ação desabilitada e texto explícito.
- Sala aberta: participantes à esquerda; telas e estado vazio no centro.
  A ação Assistir fica visível; a seleção permanece destacada enquanto assistida.
- Barra inferior: estado do próprio compartilhamento, qualidade selecionada,
  Qualidade e Compartilhar/Parar. Durante o compartilhamento, “N assistindo” abre
  os nomes das conexões estabelecidas em um popover discreto. Ctrl+Shift+S aciona
  o mesmo fluxo do botão.
- Qualidade: perfis rápidos e áudio primeiro; ajustes numéricos e medição local
  em Ajustes avançados. Painel com rolagem para telas de menor altura.

## Linguagem visual

Superfícies cinza frias: #17181e, #202129 e #262830. Palco #111217.
Acento #a7b3ff reservado à ação principal, seleção e foco. Texto #f2f3f7,
secundário #b4b7c6; verde para ao vivo, vermelho para parar/erro, sempre com texto.
Fonte do sistema a 14 px, secundário 12 px, título de sala 20 px. Ícones Cairo;
nenhuma fonte remota, biblioteca visual nova ou animação contínua.

## Estados e limites

O palco vazio ensina a compartilhar/assistir e muda enquanto a própria tela
estiver ao vivo. O botão central acompanha a disponibilidade da ação inferior.
Vídeos recebidos preservam seus widgets ao atualizar a sala e redimensionar.
Tela cheia usa o monitor atual da janela, fundo preto e preenchimento sem
deformação, com corte central se necessário. Um controle permite ver a imagem
inteira; controles se ocultam após inatividade. Não há marca gráfica no cabeçalho.
Erros são selecionáveis; nomes longos têm reticências e tooltip.

Ajuste automático inicia desligado. Remover Discord inicia ligado no Windows;
no Linux fica indisponível, explicitamente identificado. Preferências r9 recebem
esses padrões uma vez; escolhas posteriores são persistidas.

Validar em desktop real: 960×540 e 1280×760, escala 100/125/150%, teclado,
busca vazia/com resultados, sala cheia, entrada com senha lembrada, iniciar/parar,
reconectar e vídeos simultâneos/tela cheia. Compilação e parser CSS sem display
não substituem essa validação visual.
