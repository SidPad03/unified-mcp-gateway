# The build stage is pinned to $BUILDPLATFORM: `npm run build` produces static
# files that are identical whatever the target architecture, so there is no
# reason to emulate it. Only the nginx runtime differs per platform, and it has
# no RUN steps — so the arm64 image costs essentially nothing to produce on an
# x86_64 runner.
FROM --platform=$BUILDPLATFORM node:20-alpine AS builder
ARG APP_VERSION=0.0.0
WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm ci
COPY . .
ENV VITE_APP_VERSION=${APP_VERSION}
RUN npm run build

FROM nginx:alpine AS runtime
COPY --from=builder /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
