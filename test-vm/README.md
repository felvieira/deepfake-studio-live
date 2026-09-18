# Windows descartável para testar o instalador

Testar o instalador na máquina de desenvolvimento não prova nada: aqui o
Python, o `ffmpeg`, o driver da câmera e os modelos já existem, então uma
instalação pode passar por acidente. Esta VM começa vazia toda vez.

## Subir

```bash
cd test-vm && docker compose -p dlc-test up -d
```

Abra <http://localhost:8006>. A primeira execução instala o Windows sozinha e
leva bastante tempo (a ISO passa de 5 GB); as seguintes sobem em segundos,
porque o disco fica num volume.

O instalador aparece dentro do Windows como unidade **Z:**.

## Descartar

```bash
docker compose -p dlc-test down -v
```

O `-v` apaga o volume. O próximo `up` começa de um Windows limpo — que é o
ponto: cada teste parte do mesmo estado.

## O que verificar

O roteiro abaixo cobre o que só aparece numa máquina limpa. Marcar tudo é o
que autoriza dizer que o instalador funciona.

1. **Baixa do Release, não do disco.** Rodando de `Z:`, sem repositório na VM,
   o passo "Aplicativo" deve dizer *Baixando v0.1.0…*. Se disser "já
   instalado" ou usar código local, o `find_checkout` está achando algo que
   não devia.
2. **Sem privilégio de administrador**, exceto no passo da câmera virtual.
   Qualquer UAC antes disso é regressão.
3. **Retomada.** Corte a rede no meio do download dos modelos (desconecte o
   adaptador). O passo falha com mensagem clara; reconecte, clique de novo e
   ele deve continuar de onde parou — não recomeçar do zero. Confira o
   tamanho dos `.part` em `%LOCALAPPDATA%\DeepLiveCam\app\models`.
4. **Acelerador.** A VM não tem GPU NVIDIA, então o esperado é terminar com
   *rodando em CPU — a GPU não foi detectada*. Se disser CUDA aqui, a
   detecção está mentindo.
5. **Recusar o UAC do driver** deve deixar a instalação **concluir** com o
   passo da câmera em amarelo, não falhar tudo.
6. **O app abre.** Botão "Abrir" sobe a janela do PySide6. Em CPU a troca de
   rosto fica lenta — isso é esperado, não é defeito.
7. **Espaço em disco.** Com menos de ~4 GB livres, o instalador deve recusar
   **antes** de baixar qualquer coisa.

Se algo falhar, `%LOCALAPPDATA%\DeepLiveCam\logs\install.log` tem o passo a
passo — o botão "Ver log" abre direto.

## Requisitos do host

Precisa de `/dev/kvm`. No Windows isso significa Docker Desktop com WSL2 e
virtualização aninhada. Para conferir antes de subir:

```bash
wsl -d Ubuntu -e ls -la /dev/kvm
```

Se o arquivo não existir, o container sobe e morre — o Dockur é uma VM QEMU,
não um container Windows.
