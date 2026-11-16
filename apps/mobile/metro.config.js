// Metro runs from apps/mobile but the code it must see lives one level up: npm
// hoists `node_modules` to the repository root, and `@goble/common` is a
// workspace package linked there rather than copied into this app. Without
// `watchFolders` Metro refuses to resolve either, and the failure reads as a
// missing module instead of a configuration mistake.
const path = require("path");
const { getDefaultConfig } = require("expo/metro-config");

const projectRoot = __dirname;
const workspaceRoot = path.resolve(projectRoot, "../..");

const config = getDefaultConfig(projectRoot);

config.watchFolders = [workspaceRoot];
config.resolver.nodeModulesPaths = [
  path.resolve(projectRoot, "node_modules"),
  path.resolve(workspaceRoot, "node_modules"),
];

module.exports = config;
