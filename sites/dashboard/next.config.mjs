import path from 'node:path';

export default {
  reactStrictMode: true,
  turbopack: { root: path.resolve(import.meta.dirname, '../..') },
};
