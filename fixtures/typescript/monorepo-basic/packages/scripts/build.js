const path = require("path");
const { join } = require("path");

function outputDir(root) {
  return join(root, "dist");
}

module.exports = { outputDir, base: path.basename(__dirname) };
