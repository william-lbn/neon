# Test infrastructure only. Match the upstream Neon Compose server release.
FROM golang:1.19.13-bullseye AS server-build
WORKDIR /src
RUN git init . && git remote add origin https://github.com/minio/minio.git \
    && git fetch --depth=1 origin a8332efa948fd84507158c1d95df33116ee58a5a \
    && git checkout --detach FETCH_HEAD
RUN CGO_ENABLED=0 go build -mod=readonly -p 2 -trimpath \
    -ldflags='-s -w -X main.Version=RELEASE.2022-10-20T00-55-09Z -X main.CommitID=a8332efa948fd84507158c1d95df33116ee58a5a' -o /out/minio .

FROM golang:1.19.13-bullseye AS client-build
WORKDIR /src
RUN git init . && git remote add origin https://github.com/minio/mc.git \
    && git fetch --depth=1 origin ad254a8fe2c7a72c3ae8a1a0ec44b799d9f6ef9a \
    && git checkout --detach FETCH_HEAD
RUN CGO_ENABLED=0 go build -mod=readonly -p 2 -trimpath \
    -ldflags='-s -w -X main.Version=RELEASE.2022-10-20T23-26-33Z -X main.CommitID=ad254a8fe2c7a72c3ae8a1a0ec44b799d9f6ef9a' -o /out/mc .

FROM alpine:3.22.1 AS server
COPY --from=server-build /out/minio /usr/bin/minio
COPY --from=server-build /src/LICENSE /usr/share/licenses/minio/LICENSE
ENTRYPOINT ["/usr/bin/minio"]
CMD ["server", "/data"]

FROM alpine:3.22.1 AS client
COPY --from=client-build /out/mc /usr/bin/mc
COPY --from=client-build /src/LICENSE /usr/share/licenses/mc/LICENSE
ENTRYPOINT ["/usr/bin/mc"]
