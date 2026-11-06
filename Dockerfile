# The game server and the page it serves. The page is built beforehand, because building it takes a quarter
# of an hour and a lot of memory: FRONTCRAFT_ONLINE=1 scripts/build_web.sh dist/web. Nothing in the server
# needs Bevy, so none of that gets built here.
FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p prs_server --bin prs_server && strip bin/release/prs_server

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=build /src/bin/release/prs_server ./prs_server
COPY --from=build /src/mods ./mods
COPY --from=build /src/dist/web ./web
ENV PRS_WEB_DIR=/app/web
ENV PRS_MAP=town
ENV PRS_MODE=aas
ENV PORT=7777
EXPOSE 7777
CMD ["./prs_server"]
