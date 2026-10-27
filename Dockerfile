# One Dockerfile for every app in the monorepo: `docker build --build-arg APP=web .`
# Multi-arch friendly (amd64 + arm64; the Raspberry Pi 5 nodes run arm64).
FROM node:22-alpine AS build
ARG APP
WORKDIR /repo
# Whole workspace: npm ci needs every workspace manifest the lockfile mentions.
COPY . .
RUN npm ci --no-audit --no-fund \
 && npm run build -w apps/${APP} \
 && npm prune --omit=dev

FROM node:22-alpine
ARG APP
ENV NODE_ENV=production PORT=3000
WORKDIR /app
COPY --from=build /repo/node_modules ./node_modules
COPY --from=build /repo/apps/${APP}/dist ./dist
COPY --from=build /repo/apps/${APP}/package.json ./package.json
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- http://localhost:3000/healthz || exit 1
CMD ["node", "dist/server.js"]
