import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { MODELS } from "../src/models.generated.ts";

const outputPath = resolve(import.meta.dirname, "../../../rust/assets/models.json");
const checkOnly = process.argv.includes("--check");

function sortJson(value: unknown): unknown {
	if (Array.isArray(value)) {
		return value.map(sortJson);
	}
	if (value !== null && typeof value === "object") {
		const source = value as Record<string, unknown>;
		const sorted: Record<string, unknown> = {};
		for (const key of Object.keys(source).sort()) {
			sorted[key] = sortJson(source[key]);
		}
		return sorted;
	}
	return value;
}

const providers: Record<string, unknown> = {};
for (const [providerId, models] of Object.entries(MODELS).sort(([left], [right]) => left.localeCompare(right))) {
	providers[providerId] = {
		id: providerId,
		models,
	};
}

const content = `${JSON.stringify(sortJson({ schemaVersion: 1, providers }), null, 2)}\n`;
if (checkOnly) {
	if (!existsSync(outputPath) || readFileSync(outputPath, "utf8") !== content) {
		throw new Error(`Rust model catalog is stale: run npm run generate:rust-model-catalog`);
	}
} else {
	writeFileSync(outputPath, content, "utf8");
}
