import { dirname } from "node:path";
import { SessionManager } from "../../../packages/coding-agent/src/core/session-manager.ts";

const [command, sessionPath] = process.argv.slice(2);
if (!command || !sessionPath) {
	throw new Error("usage: session-interop.ts <read|append> <session-path>");
}

const session = SessionManager.open(sessionPath, dirname(sessionPath));
if (command === "append") {
	session.appendCustomEntry("typescript_append", { source: "typescript" });
}
if (command !== "read" && command !== "append") {
	throw new Error(`unsupported command: ${command}`);
}

process.stdout.write(
	`${JSON.stringify({
		sessionId: session.getSessionId(),
		entries: session.getEntries().map((entry) => ({
			type: entry.type,
			customType: "customType" in entry ? entry.customType : undefined,
		})),
	})}\n`,
);
