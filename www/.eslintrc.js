module.exports = {
  env: {
    browser: true,
    es2021: true,
  },
  extends: [
    'airbnb-base',
  ],
  parserOptions: {
    ecmaVersion: 13,
    sourceType: 'module',
  },
  rules: {
    'max-classes-per-file': 'off',
    'no-nested-ternary': 'off',
    'no-use-before-define': ['error', { functions: false, classes: true, variables: true }],
    'no-console': 'off',
  },
};
