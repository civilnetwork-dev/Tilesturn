import { defineConfig } from "vitest/config";

export default defineConfig({
    test: {
        environment: "node",
        // Worker threads run cleanly on both Node and Bun (Bun lacks
        // `child.channel.unref()` needed by Vitest's default forks pool).
        pool: "threads",
        include: ["tests/**/*.test.ts"],
    },
});
