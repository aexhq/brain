import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  output: 'export',
  trailingSlash: true,
  reactStrictMode: true,
  basePath: process.env.NEXT_PUBLIC_BASE_PATH ?? '',
};

export default withMDX(config);
