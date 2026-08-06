FROM node:22-slim
WORKDIR /app
COPY package.json .npmrc* ./
RUN npm install express --no-audit --no-fund
COPY index.js ./
EXPOSE 8080
CMD ["node", "index.js"]
