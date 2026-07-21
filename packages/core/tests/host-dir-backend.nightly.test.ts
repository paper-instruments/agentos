// Nightly: projects the complete registry command bundle.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import coreutils from "@agentos-software/coreutils";
import grep from "@agentos-software/grep";
import sed from "@agentos-software/sed";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

describe("host_dir native mount integration", () => {
	let vm: AgentOs;
	let tmpDir: string;
	let outsideDir: string;

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "host-dir-test-"));
		outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "host-dir-outside-"));
		fs.writeFileSync(path.join(tmpDir, "hello.txt"), "hello from host");
		fs.mkdirSync(path.join(tmpDir, "subdir"));
		fs.writeFileSync(
			path.join(tmpDir, "subdir", "nested.txt"),
			"nested content",
		);
	});

	afterEach(async () => {
		if (vm) await vm.dispose();
		fs.rmSync(tmpDir, { recursive: true, force: true });
		fs.rmSync(outsideDir, { recursive: true, force: true });
	});

	test("path traversal attempt (../../etc/passwd) is blocked", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir }),
				},
			],
		});
		await expect(vm.readFile("/hostmnt/../../etc/passwd")).rejects.toThrow();
	});

	test("mounted host directory exposes existing host files", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir }),
				},
			],
		});
		const content = new TextDecoder().decode(
			await vm.readFile("/hostmnt/hello.txt"),
		);
		expect(content).toBe("hello from host");
	});

	test("mounted host directory is readable from guest exec", async () => {
		vm = await AgentOs.create({
			permissions: {
				fs: "allow",
				network: "deny",
				childProcess: "allow",
				process: "allow",
				env: "allow",
				binding: "allow",
			},
			defaultSoftware: false,
			software: [coreutils],
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir }),
				},
			],
		});
		const result = await vm.exec("cat /hostmnt/hello.txt");
		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stdout).toContain("hello from host");
	});

	test("guest exec writes a private writable host directory", async () => {
		if (process.platform !== "win32") fs.chmodSync(tmpDir, 0o700);
		vm = await AgentOs.create({
			permissions: {
				fs: "allow",
				network: "deny",
				childProcess: "allow",
				process: "allow",
				env: "allow",
				binding: "allow",
			},
			defaultSoftware: false,
			software: [coreutils],
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir, readOnly: false }),
				},
			],
		});
		const result = await vm.exec(
			"printf written > /hostmnt/from-guest.txt && cat /hostmnt/from-guest.txt",
		);
		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stdout).toBe("written");
		expect(fs.readFileSync(path.join(tmpDir, "from-guest.txt"), "utf8")).toBe(
			"written",
		);
	});

	test("Bash waits for an external pipeline redirected into a host mount", async () => {
		if (process.platform !== "win32") fs.chmodSync(tmpDir, 0o700);
		vm = await AgentOs.create({
			permissions: {
				fs: "allow",
				network: "deny",
				childProcess: "allow",
				process: "allow",
				env: "allow",
				binding: "allow",
			},
			defaultSoftware: false,
			software: [coreutils, grep, sed],
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir, readOnly: false }),
				},
			],
		});
		fs.writeFileSync(path.join(tmpDir, "direct-input.txt"), "alpha\nbeta\n");

		const { pid } = vm.spawn(
			"bash",
			[
				"-lc",
				"set -euo pipefail; grep beta direct-input.txt > direct-output.txt; printf 'alpha\\nbeta\\n' | grep beta | sed 's/beta/BETA/' > result.txt; for value in 1 2 3; do printf '%s' \"$value\"; done",
			],
			{
				cwd: "/hostmnt",
				env: { HOME: "/hostmnt", PWD: "/hostmnt" },
				stdio: "pipe",
			},
		);
		let stdout = "";
		let stderr = "";
		vm.onProcessOutput(pid, (event) => {
			const text = new TextDecoder().decode(event.data);
			if (event.stream === "stdout") stdout += text;
			else stderr += text;
		});
		await vm.closeProcessStdin(pid);
		const exitCode = await vm.waitProcess(pid);
		const directHostAtExit = fs.readFileSync(
			path.join(tmpDir, "direct-output.txt"),
			"utf8",
		);
		const pipelineHostAtExit = fs.readFileSync(
			path.join(tmpDir, "result.txt"),
			"utf8",
		);
		const directGuestAtExit = new TextDecoder().decode(
			await vm.readFile("/hostmnt/direct-output.txt"),
		);
		const pipelineGuestAtExit = new TextDecoder().decode(
			await vm.readFile("/hostmnt/result.txt"),
		);
		await new Promise((resolve) => setTimeout(resolve, 500));
		const directHostAfterDelay = fs.readFileSync(
			path.join(tmpDir, "direct-output.txt"),
			"utf8",
		);
		const pipelineHostAfterDelay = fs.readFileSync(
			path.join(tmpDir, "result.txt"),
			"utf8",
		);
		if (directHostAtExit !== "beta\n" || pipelineHostAtExit !== "BETA\n") {
			console.error(
				"redirect diagnostics",
				JSON.stringify({
					directHostAtExit,
					pipelineHostAtExit,
					directGuestAtExit,
					pipelineGuestAtExit,
					directHostAfterDelay,
					pipelineHostAfterDelay,
				}),
			);
		}

		expect(exitCode, stderr || stdout).toBe(0);
		expect(stdout).toBe("123");
		expect(directHostAtExit).toBe("beta\n");
		expect(pipelineHostAtExit).toBe("BETA\n");
	});

	test("symlink escape attempt is blocked", async () => {
		const escapePath = path.join(tmpDir, "escape");
		fs.writeFileSync(path.join(outsideDir, "secret.txt"), "host secret");
		fs.symlinkSync(
			outsideDir,
			escapePath,
			process.platform === "win32" ? "junction" : "dir",
		);

		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir }),
				},
			],
		});
		await expect(vm.readFile("/hostmnt/escape/secret.txt")).rejects.toThrow(
			"EACCES",
		);
	});

	test.runIf(process.platform === "win32")(
		"Windows drive paths are not guest-visible host paths",
		async () => {
			vm = await AgentOs.create({
				defaultSoftware: false,
				mounts: [
					{
						path: "/hostmnt",
						plugin: createHostDirBackend({ hostPath: tmpDir }),
					},
				],
			});
			await expect(vm.readFile("C:\\Windows\\win.ini")).rejects.toThrow();
		},
	);

	test("write blocked when helper defaults to readOnly", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir }),
				},
			],
		});
		await expect(
			vm.writeFile("/hostmnt/new.txt", "should fail"),
		).rejects.toThrow("EROFS");
	});

	test("write works when readOnly: false", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir, readOnly: false }),
				},
			],
		});
		await vm.writeFile("/hostmnt/writable.txt", "written from VM");

		// Verify on host
		const content = fs.readFileSync(path.join(tmpDir, "writable.txt"), "utf-8");
		expect(content).toBe("written from VM");
	});

	test("rename and delete update the host directory when writable", async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			mounts: [
				{
					path: "/hostmnt",
					plugin: createHostDirBackend({ hostPath: tmpDir, readOnly: false }),
				},
			],
		});

		await vm.writeFile("/hostmnt/to-rename.txt", "rename me");
		await vm.move("/hostmnt/to-rename.txt", "/hostmnt/renamed.txt");
		expect(fs.existsSync(path.join(tmpDir, "to-rename.txt"))).toBe(false);
		expect(fs.readFileSync(path.join(tmpDir, "renamed.txt"), "utf-8")).toBe(
			"rename me",
		);

		await vm.remove("/hostmnt/renamed.txt");
		expect(fs.existsSync(path.join(tmpDir, "renamed.txt"))).toBe(false);
	});
});
