import js from "@eslint/js";

export default [
  js.configs.recommended,
  {
    ignores: ["dist/", "wasm/pkg/"],
  },
  {
    files: ["webpack.config.js"],
    languageOptions: {
      sourceType: "commonjs",
      globals: {
        require: "readonly",
        module: "writable",
        exports: "writable",
        __dirname: "readonly",
        __filename: "readonly",
        process: "readonly",
      },
    },
  },
  {
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: {
        window: "readonly",
        document: "readonly",
        console: "readonly",
        URLSearchParams: "readonly",
        URL: "readonly",
        history: "readonly",
        location: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        requestAnimationFrame: "readonly",
        ResizeObserver: "readonly",
        MutationObserver: "readonly",
        HTMLElement: "readonly",
        Event: "readonly",
        CustomEvent: "readonly",
        fetch: "readonly",
        AbortController: "readonly",
        navigator: "readonly",
      },
    },
    rules: {
      "no-use-before-define": [
        "error",
        { functions: false, classes: true, variables: true },
      ],
      "no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
];
