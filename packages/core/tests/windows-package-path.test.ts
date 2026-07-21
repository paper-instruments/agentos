import { describe, expect, test } from "vitest";
import { normalizePackageHostPath } from "../src/agent-os.js";

describe("Windows package host paths", () => {
	test("converts file URL pathnames into native drive paths", () => {
		expect(
			normalizePackageHostPath(
				"/D:/Program%20Files/AgentOS/package.aospkg",
				"win32",
			),
		).toBe("D:\\Program Files\\AgentOS\\package.aospkg");
	});

	test("does not reinterpret paths on other platforms", () => {
		expect(normalizePackageHostPath("/opt/agentos/package.aospkg", "linux")).toBe(
			"/opt/agentos/package.aospkg",
		);
	});
});
