FROM node:26-bookworm-slim@sha256:bc0a2afd3b74ce12222fc8bab99271e66257f021078f5f4a5d687a77d280c1d9
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
