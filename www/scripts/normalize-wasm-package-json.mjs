import fs from "node:fs";

const packageJsonUrl = new URL("../wasm/pkg/package.json", import.meta.url);

if (fs.existsSync(packageJsonUrl)) {
	const pkg = JSON.parse(fs.readFileSync(packageJsonUrl, "utf8"));
	if (typeof pkg.repository !== "string") {
		pkg.repository =
			pkg.repository?.url || "https://github.com/aymericbeaumet/gors/";
		fs.writeFileSync(packageJsonUrl, `${JSON.stringify(pkg, null, 2)}\n`);
	}
}
