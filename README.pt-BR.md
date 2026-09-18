<h1 align="center">Deep-Live-Cam — Edição Câmera Virtual</h1>

<p align="center">
  Troca de rosto em tempo real que publica direto numa câmera virtual — sem precisar de OBS.
</p>

<p align="center">
  <a href="README.md">English</a> · <b>Português (Brasil)</b>
</p>

<p align="center">
  <img src="media/demo.gif" alt="Demo GIF" width="800">
</p>

---

## Sobre este fork

Este é um fork independente do [hacksider/Deep-Live-Cam](https://github.com/hacksider/Deep-Live-Cam).
Todo o trabalho de troca de rosto — o pipeline, os modelos, o ajuste de performance — vem
daquele projeto e das pessoas creditadas no fim desta página. Este fork não reivindica nada disso.

O que este fork acrescenta:

| Mudança | Por que importa |
|---|---|
| **Câmera virtual integrada** | O projeto original manda você capturar a janela de preview com o OBS. Aqui os frames processados vão direto para uma câmera virtual que o Zoom, Discord, Teams e Meet enxergam como webcam comum. |
| **Interface reconstruída** | Arquivo e Ao vivo agora são dois modos separados, em vez de um painel só cheio de coisa. As opções avançadas ficam recolhidas, então o caminho comum é curto. |
| **Instalação em um passo** | O `install.bat` cria o ambiente, instala as dependências e registra o driver da câmera. |
| **Driver UnityCapture incluído** | O driver vem em `third_party/`, então não tem nada pra caçar por fora. |

O resto funciona igual ao original. Se você quer o projeto original, vá lá — é a implementação
de referência e tem manutenção ativa.

---

## Aviso importante

Isto é software de deepfake. Leia esta parte.

**Consentimento não é opcional.** Se você usar o rosto de uma pessoa real, peça permissão antes.
Não é formalidade — em muitos lugares usar a imagem de alguém sem consentimento é crime, e
"era só brincadeira" não serve de defesa.

**Identifique o que você publicar.** Ao compartilhar algo feito com esta ferramenta, diga que é
deepfake. As pessoas têm o direito de saber se o que estão vendo é real.

**O filtro NSFW fica.** O projeto tem uma verificação que bloqueia material impróprio.
Não remova. Se remover, o problema é seu — inclusive o jurídico.

**Uso não comercial apenas.** Os modelos de análise facial (InsightFace `buffalo_l`) são
licenciados [somente para pesquisa não comercial](https://github.com/deepinsight/insightface?tab=readme-ov-file#license).
Essa restrição vem dos autores dos modelos, não da licença deste projeto, e vale independente
do que você faça com o código.

Você é responsável pelo que criar com isto. Não os autores deste fork, nem o projeto original.

---

## Instalação

**Windows**

```bash
install.bat
```

Isso cria o ambiente virtual, instala as dependências Python e registra o driver UnityCapture.
O Windows vai pedir permissão de Administrador na etapa do driver — só essa etapa precisa;
o resto não.

Precisa de Python 3.11–3.13 e `ffmpeg` no PATH.

A primeira execução baixa cerca de 1 GB de modelos do
[Hugging Face](https://huggingface.co/hacksider/deep-live-cam). A janela vai parecer travada
enquanto isso acontece — o progresso só aparece no terminal. Se a conexão cair, o download
continua de onde parou.

**Linux / macOS**

Siga o [guia de instalação do projeto original](https://github.com/hacksider/Deep-Live-Cam#installation-manual).
A câmera virtual deste fork é só Windows; o resto funciona normalmente.

---

## Usando a câmera virtual

1. Rode `python run.py`
2. Escolha o modo **Live camera**
3. Selecione uma imagem com o rosto de origem
4. Ligue **Send video to a virtual camera**
5. Clique em **Start live**
6. No Zoom, Discord, Teams ou Meet, selecione **Unity Video Capture** como câmera

Se a câmera não aparecer, reinicie o app da chamada — a maioria lista os dispositivos só uma
vez, quando abre.

Para remover o driver depois: `uninstall_virtual_camera.bat`.

---

## Aceleração por GPU

CUDA, DirectML, OpenVINO e CoreML funcionam como documentado no projeto original. Veja o
[guia de GPU do original](https://github.com/hacksider/Deep-Live-Cam#gpu-acceleration) para os
comandos de instalação de cada provider.

O cabeçalho mostra qual acelerador está ativo. Se aparecer **CPU mode** numa máquina com GPU
compatível, o provider não carregou — quase sempre é incompatibilidade de versão entre CUDA e
cuDNN. O app continua funcionando, só que devagar.

---

## Licença

**AGPL-3.0**, herdada do projeto original. Resumindo: você pode usar, modificar e redistribuir,
mas o que você distribuir também tem que ser AGPL e tem que vir com o código-fonte. Se rodar
uma versão modificada como serviço de rede, você deve o fonte aos usuários também.

O texto completo está em [LICENSE](LICENSE). A restrição não comercial do InsightFace citada no
aviso vale por cima dela.

---

## Créditos

Este fork existe por causa de trabalho feito em outro lugar. O crédito é de:

- [hacksider](https://github.com/hacksider) e os [contribuidores do Deep-Live-Cam](https://github.com/hacksider/Deep-Live-Cam/graphs/contributors) — o projeto que este fork deriva
- [s0md3v](https://github.com/s0md3v/roop) — o roop original, de onde tudo isso descende
- [deepinsight](https://github.com/deepinsight) — [InsightFace](https://github.com/deepinsight/insightface), os modelos de análise facial
- [schellingb](https://github.com/schellingb/UnityCapture) — UnityCapture, o driver da câmera virtual
- [ffmpeg](https://ffmpeg.org/) — operações de vídeo
- [Henry](https://github.com/henryruhs) — contribuidor principal do projeto original

Citados no README do original pelas contribuições que fizeram lá:
[havok2-htwo](https://github.com/havok2-htwo),
[GosuDRM](https://github.com/GosuDRM),
[pereiraroland26](https://github.com/pereiraroland26),
[vic4key](https://github.com/vic4key),
[kier007](https://github.com/kier007),
[qitianai](https://github.com/qitianai),
[laurigates](https://github.com/laurigates),
[maxwbuckley](https://github.com/maxwbuckley).
