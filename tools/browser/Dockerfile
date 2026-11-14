FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
WORKDIR /app/tools/browser
COPY tools/browser/package*.json ./
RUN npm ci --ignore-scripts
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
RUN node node_modules/playwright/cli.js install --with-deps chromium
COPY tools/http.mjs /app/tools/http.mjs
COPY tools/browser/src ./src
COPY tools/browser/test ./test
USER node
RUN npm test
EXPOSE 3005
CMD ["node", "src/main.mjs"]
