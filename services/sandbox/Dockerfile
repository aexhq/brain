FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
WORKDIR /app/services/sandbox
COPY services/sandbox/package*.json ./
RUN npm ci --omit=dev --ignore-scripts
COPY services/http.mjs /app/services/http.mjs
COPY services/sandbox/src ./src
USER node
ENV HOST=0.0.0.0
EXPOSE 8091
CMD ["node", "src/server.mjs"]
