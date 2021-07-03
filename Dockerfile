FROM i386/gcc:6.1 AS sourcemod
WORKDIR /gcc/src/github.com/alliedmodders/
RUN git clone https://github.com/alliedmodders/sourcemod/ --recursive
RUN apt-get update && apt-get install -y gcc-multilib
RUN LIBRARY_PATH=/usr/lib32:$LIBRARY_PATH && export LIBRARY_PATH
RUN cd sourcemod/tools/gdc-psyfork && make

FROM rust:1.53 AS ambot
WORKDIR /ambot/
COPY . .
RUN cargo build --release

FROM mcr.microsoft.com/dotnet/runtime:5.0
LABEL Name=ambot Version=0.1.1
WORKDIR /root/
COPY --from=sourcemod /gcc/src/github.com/alliedmodders/sourcemod/ sourcemod/
COPY --from=ambot /ambot/target/release/ambot ambot
RUN apt-get update \
  && apt-get install -y wget curl unzip git lib32stdc++6 \
  && rm -rf /var/lib/apt/lists/*
# Install depotdownloader
RUN wget https://github.com/SteamRE/DepotDownloader/releases/download/DepotDownloader_2.4.3/depotdownloader-2.4.3-hotfix1.zip -O depotdownloader.zip \
  && unzip depotdownloader.zip -d ./depotdownloader/ \
  && rm depotdownloader.zip
RUN mkdir downloads
RUN mkdir gamedata
ENV SOURCEMOD_DIR=/root/sourcemod
ENV DEPOT_DIR=/root/depotdownloader
ENV DOWNLOADS_DIR=/root/downloads
ENTRYPOINT [ "./ambot" ]
