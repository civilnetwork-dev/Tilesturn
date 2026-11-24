import { defineConfig } from "oxlint";

/**
 * Mirrors ../Civil's oxlint configuration for non-JSX TypeScript packages.
 * Formatting and import sorting are delegated to oxfmt.config.ts.
 */
export default defineConfig({
    plugins: ["typescript", "unicorn", "oxc", "import", "promise"],
    categories: {
        correctness: "error",
        suspicious: "warn",
        perf: "warn",
    },
    ignorePatterns: ["dist", "pkg", "target", "profiles", "**/fixtures/**"],
    rules: {
        // The WASM and JS boundary carries dynamic JSON payloads and probe
        // answers whose inner shape is determined by Cloudflare's runtime.
        "typescript/no-explicit-any": "off",
        "typescript/no-non-null-assertion": "off",
        "typescript/no-non-null-asserted-optional-chain": "off",
        // Namespacing helpers or constants on a class stays legal.
        "typescript/no-extraneous-class": "off",
        "typescript/no-unused-vars": [
            "warn",
            { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
        ],
        "unicorn/no-useless-switch-case": "off",
        "unicorn/prefer-node-protocol": "warn",
        // String concatenation stays legal, matching ../Civil.
        "prefer-template": "off",
        // `_`-prefixed internals are a convention for private/unused fields.
        "no-underscore-dangle": "off",
        // Sequential awaits in a loop are intentional for ordered redirect hops
        // and WebSocket echo rounds.
        "no-await-in-loop": "off",
        "unicorn/consistent-function-scoping": "off",
        "import/no-unassigned-import": "off",
        // Fixed-size object spread updates allocate once per element either way.
        "oxc/no-map-spread": "off",
        // Terminal then() callbacks doing side effects do not starve downstream links.
        "promise/always-return": ["warn", { ignoreLastCallback: true }],
        // One-shot handlers on freshly created WebSocket instances bridge events
        // to promises without leaking duplicate listeners.
        "unicorn/prefer-add-event-listener": "off",
    },
    overrides: [
        {
            files: ["tests/**", "**/*.test.ts"],
            rules: {
                "typescript/no-unused-vars": "off",
            },
        },
    ],
});
