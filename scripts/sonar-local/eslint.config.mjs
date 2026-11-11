// Local stand-ins for SonarCloud's TypeScript/TSX rules (scripts/sonar-local.sh). Only bug and
// code-smell rules Sonar reports on this repository are switched on, so a clean run here means
// the same files should not turn the quality gate red.
import tsParser from "@typescript-eslint/parser";
import jsxA11y from "eslint-plugin-jsx-a11y";
import react from "eslint-plugin-react";
import sonarjs from "eslint-plugin-sonarjs";

export default [
  {
    files: ["**/*.ts", "**/*.tsx"],
    languageOptions: {
      parser: tsParser,
      parserOptions: { ecmaFeatures: { jsx: true }, sourceType: "module" },
    },
    plugins: { sonarjs, "jsx-a11y": jsxA11y, react },
    settings: { react: { version: "18.3" } },
    rules: {
      // S3776 — Sonar's limit for TypeScript is 15.
      "sonarjs/cognitive-complexity": ["error", 15],
      // S8786-like: regular expressions with super-linear backtracking.
      "sonarjs/slow-regex": "error",
      "sonarjs/no-identical-functions": "error",
      "sonarjs/no-all-duplicated-branches": "error",
      "sonarjs/no-identical-conditions": "error",
      "sonarjs/no-collapsible-if": "error",
      "sonarjs/prefer-single-boolean-return": "error",
      "sonarjs/no-redundant-jump": "error",
      "sonarjs/no-small-switch": "error",
      "sonarjs/no-unused-collection": "error",
      "sonarjs/no-dead-store": "error",
      // S6847, S1082: click handlers only on interactive elements, with a keyboard path.
      "jsx-a11y/no-noninteractive-element-interactions": "error",
      "jsx-a11y/no-static-element-interactions": "error",
      "jsx-a11y/click-events-have-key-events": "error",
      "jsx-a11y/label-has-associated-control": ["error", { assert: "either" }],
      // S6772: text right after a JSX element on the next line.
      "react/jsx-child-element-spacing": "error",
      "react/jsx-key": "error",
    },
  },
];
