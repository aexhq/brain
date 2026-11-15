FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
WORKDIR /app/tools/sandbox
COPY tools/sandbox/package*.json ./
RUN npm ci --omit=dev --ignore-scripts
COPY tools/http.mjs /app/tools/http.mjs
COPY tools/sandbox/src ./src
USER node
ENV HOST=0.0.0.0
EXPOSE 8091
CMD ["node", "src/server.mjs"]
