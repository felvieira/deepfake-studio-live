# Situação do projeto

Deepfake Studio Live é um fork do [Deep-Live-Cam](https://github.com/hacksider/Deep-Live-Cam)
(AGPL-3.0). Este documento registra o que foi feito, o que foi testado e o que falta.

Última versão publicada: [v0.2.1](https://github.com/felvieira/deepfake-studio-live/releases/tag/v0.2.1),
com instalador para Windows (`DeepfakeStudioLive-Setup.exe`) e macOS
(`DeepfakeStudioLive-macOS.dmg`).

Telas e opções do app e do instalador: [telas.html](telas.html).

## O que foi feito

### Instalador

- Instalador próprio (Tauri) para Windows e macOS. Ele baixa o ambiente, as dependências, o app
  e os 9 modelos de IA (1,3 GB), com verificação de espaço (9,6 GB) antes de começar.
- Progresso por etapa, com porcentagem, velocidade e tempo restante.
- As mensagens não citam a tecnologia usada: dizem "Configurando o ambiente", por exemplo.
- Downloads retomam de onde pararam, e os principais têm conferência de SHA256.
- Atalho "Deepfake Studio Live" que abre o app direto, sem passar pelo instalador.
- No macOS, um app lançador em `~/Applications` faz esse papel e pede a permissão de câmera.
- Instala as bibliotecas CUDA quando há placa NVIDIA.
- Atualização: o botão vira **Atualizar** quando a versão do instalador muda, e o código do app é
  baixado de novo quando sai um release novo.
- Atualização com arquivo em uso: o filtro da câmera virtual fica carregado por qualquer programa
  que lista câmeras. Agora ele é renomeado e substituído, e a sobra sai na atualização seguinte.
- Modo sem janela (`--install-headless`), usado no CI.

### App

- Marca própria: nome, ícone, título da janela e barra de tarefas.
- Tela de carregamento enquanto o app abre.
- Interface com modos Arquivo e Live separados e opções avançadas recolhidas.
- Interruptores redesenhados e estados de carregamento.
- Selo que mostra a GPU encontrada e se ela está mesmo em uso.
- Botão Start live que vira Stop live, e botão para liberar a memória da GPU. Parar o live também
  libera a memória, a menos que a opção de manter os modelos esteja ligada.
- Câmera virtual integrada: Unity Capture no Windows, OBS no macOS, v4l2loopback no Linux.
- Idioma detectado pelo sistema.
- ffmpeg incluído, com leitura de vídeo pelo OpenCV quando não há ffprobe.
- Mensagem de erro em janela quando o app não consegue abrir.

### macOS

- Python próprio para Apple Silicon, conferência das ferramentas de linha de comando do Xcode e
  detecção do OBS.
- CoreML para os modelos e VideoToolbox para codificar vídeo, com recurso ao software.
- Listagem de câmeras e nome do chip no selo.
- Instalação pelo código-fonte com `install.sh` e `run.sh`.

### Repositório e CI

- Repositório público, READMEs em inglês e português.
- `verify.yml`: instala do zero em Windows e macOS, roda o teste de fumaça e abre o app pelo atalho.
- `release.yml`: gera os dois instaladores e publica o release quando recebe uma tag.
- `ruff.yml`: lint do Python.
- Limpeza desta entrega: código da interface Tk antiga, `mypi.ini` (nome errado, nunca lido) e os
  65 MB de GIFs e imagens do projeto original que nada usava. Continuam no histórico do git.

## O que foi testado

| Item | Windows | macOS |
|---|---|---|
| Instalação do zero | Em máquina real e no CI | CI (Mac M1 virtual) |
| Abrir pelo atalho | Em máquina real e no CI | CI |
| Aceleração em uso | CUDA em RTX 5060 Ti | CoreML no CI |
| Inferência do swapper | Sim | Sim (386 ms) |
| Atualização sobre instalação antiga | Em máquina real | Não testado |
| Live com webcam de verdade | Sim | **Não testado** |
| Câmera virtual em Zoom, Discord ou Teams | **Não registrado nesta fase** | **Não testado** |
| Permissão de câmera do macOS | Não se aplica | **Não testado** |
| Primeira abertura pelo Gatekeeper | Não se aplica | **Não testado** |

O CI não tem câmera nem GPU física. Por isso os itens do macOS ainda dependem de um Mac de verdade.

## O que falta

### Prioridade alta

1. **Testar num Mac real:** download do dmg, "Abrir Mesmo Assim", instalação, permissão de câmera,
   live com webcam e câmera virtual pelo OBS.
2. **Testar a câmera virtual ponta a ponta** no Windows, numa chamada de verdade.
3. **Publicar a correção da limpeza de sobras:** a limpeza do `.dsl-old` está na `main`, mas ainda
   não está em nenhum release. O v0.2.1 só apaga a sobra se o arquivo estiver travado de novo.
   Precisa de uma versão 0.2.2.

### Prioridade média

4. **Facelift da interface:** tema claro e escuro e o visual dos mockups. Só a análise foi feita.
5. **Assinatura dos instaladores:** sem certificado, o Windows mostra o aviso do SmartScreen e o
   macOS exige "Abrir Mesmo Assim". No Mac, precisa da conta Apple Developer e de notarização.
6. **Desinstalação completa:** remover também `%LOCALAPPDATA%\DeepLiveCam` e os atalhos. Hoje esse
   fluxo não está implementado nem testado.
7. **Traduzir a interface do instalador:** o texto é só em português, e a do app tem idiomas.
8. **Revisar os PRs do Dependabot:** há vários abertos. O do `insightface` 2.0 é uma mudança de
   versão maior e pede teste de verdade.

### Prioridade baixa

9. **Macs com Intel:** o instalador só é gerado para Apple Silicon. Hoje esses Macs usam a
   instalação pelo código-fonte, que não foi testada.
10. **Linux:** sem instalador. Só há instalação pelo código-fonte, também sem teste.
11. **Mais testes automáticos do app:** existem só dois arquivos em `tests/`, e o CI não os roda.
12. **`CONTRIBUTING.md`:** ainda cita as branches `premain` e `experimental` do projeto original,
    que não existem aqui.
13. **Arquivos removidos em atualizações:** a atualização sobrescreve o app sem apagar o que saiu
    da versão nova. Não causa erro, mas deixa arquivos antigos na pasta.
14. **Branches remotas:** `feat/macos` já foi incorporada e pode ser apagada no GitHub. As demais
    vieram do projeto original ou do Dependabot.

## Como publicar uma versão

1. Atualize a versão em `installer/src-tauri/Cargo.toml` e `installer/src-tauri/tauri.conf.json`.
2. Faça o commit na `main`.
3. Crie e envie a tag:

```bash
git tag v0.2.2
git push origin v0.2.2
```

O `release.yml` gera e publica os dois instaladores.
