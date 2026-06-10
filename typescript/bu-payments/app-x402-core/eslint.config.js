import js from "@eslint/js";
import ts from "@typescript-eslint/eslint-plugin";
import tsParser from "@typescript-eslint/parser";
import prettier from "eslint-plugin-prettier";
import importPlugin from "eslint-plugin-import";

export default [
  {
    ignores: ["dist/**", "node_modules/**", "src/paywall/dist/**", "src/paywall/gen/**"],
  },
  {
    files: ["**/*.ts", "**/*.tsx"],
    ignores: ["**/*.test.ts", "test/**/*"],
    languageOptions: {
      parser: tsParser,
      sourceType: "module",
      ecmaVersion: 2020,
      globals: {
        process: "readonly",
        __dirname: "readonly",
        module: "readonly",
        require: "readonly",
        Buffer: "readonly",
        exports: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
      },
    },
    plugins: {
      "@typescript-eslint": ts,
      prettier: prettier,
      import: importPlugin,
    },
    rules: {
      ...ts.configs.recommended.rules,
      "import/first": "error",
      "prettier/prettier": "error",
      "@typescript-eslint/member-ordering": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_$" }],
    },
  },
  {
    files: ["**/*.test.ts", "test/**/*"],
    languageOptions: {
      parser: tsParser,
      sourceType: "module",
      ecmaVersion: 2020,
    },
    plugins: {
      "@typescript-eslint": ts,
      prettier: prettier,
    },
    rules: {
      "prettier/prettier": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/member-ordering": "off",
    },
  },
];
