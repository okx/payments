import { defineConfig } from "tsup";

const baseConfig = {
  entry: {
    index: "src/index.ts",
  },
  dts: { resolve: true },
  sourcemap: true,
  target: "es2022" as const,
  external: [
    "@okxweb3/mpp",
    "@okxweb3/mpp/evm",
    "@okxweb3/mpp/evm/server",
    "@okxweb3/x402-core",
    "@okxweb3/x402-core/server",
    "@okxweb3/x402-core/types",
    "mppx",
    "mppx/server",
  ],
};

export default defineConfig([
  {
    ...baseConfig,
    format: "esm",
    outDir: "dist/esm",
    clean: true,
  },
  {
    ...baseConfig,
    format: "cjs",
    outDir: "dist/cjs",
    clean: false,
  },
]);
