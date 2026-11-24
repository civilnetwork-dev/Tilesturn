import { defineConfig } from "oxfmt";

/**
 * Matches the formatting standard in ../Civil: 4-space indentation, 80-column
 * print width, LF endings, avoided single-arg arrow parens, and sorted imports.
 */
export default defineConfig({
    printWidth: 80,
    useTabs: false,
    tabWidth: 4,
    endOfLine: "lf",
    arrowParens: "avoid",
    sortImports: {},
    ignorePatterns: [
        // Markdown prose and YAML configs stay hand-laid-out.
        "**/*.md",
        "**/*.yml",
        "**/*.yaml",
        "dist",
        "pkg",
        "target",
        // Upstream tracked browser profiles keep their original formatting.
        "profiles",
    ],
});
