ARG VARIANT="edge"
FROM alpine:${VARIANT}

ARG UID="1000"
ARG USER="lapce"
RUN adduser -D ${USER} -u ${UID} && \
    addgroup ${USER} wheel

RUN apk add --no-cache doas cargo rust-src freetype-dev \
    gtk+3.0-dev libgit2-dev libssh2-dev libxcb-dev \
    libxfixes-dev libxkbcommon-dev openssl-dev python3 \
    vulkan-loader-dev wayland-dev curl wget alpine-sdk \
    zsh fish bash less coreutils util-linux

RUN echo "permit nopass :wheel" > /etc/doas.d/doas.conf

ENV OPENSSL_NO_VENDOR=1

USER ${USER}
SHELL ["bash"]
COPY .gitconfig /home/${USER}/.gitconfig