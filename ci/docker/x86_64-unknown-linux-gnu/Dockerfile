FROM japaric/x86_64-unknown-linux-gnu:latest

RUN dpkg --add-architecture amd64 && \
    apt-get update && \
    apt-get install libsdl2-dev:amd64
