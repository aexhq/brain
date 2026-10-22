FROM node:25-bookworm-slim@sha256:81db02c4b671288a03915da9534dbd54f96d0e7c24d80ccc54f5b36b2e684370
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
