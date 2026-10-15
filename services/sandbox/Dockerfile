FROM node:24-bookworm-slim
WORKDIR /app/services/sandbox
COPY services/sandbox/package*.json ./
RUN npm ci --omit=dev --ignore-scripts
COPY services/http.mjs /app/services/http.mjs
COPY services/sandbox/src ./src
USER node
ENV HOST=0.0.0.0
EXPOSE 8091
CMD ["node", "src/server.mjs"]
