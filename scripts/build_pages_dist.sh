#!/bin/bash
set -e

# Assembles a GitHub-Pages-shaped dist/ directory out of the two independent
# top-level sources:
#   dist/            <- landing/ (marketing landing page, deployed at site root)
#   dist/app/        <- web/     (the actual player app, deployed at /app/)
#
# This reshuffle only happens for the GitHub Pages artifact. Other deploy
# targets (e.g. deploy.sh -> belokan) ship web/ as-is, app at root, and never
# touch landing/ at all.
#
# Assumes `bash wasm-build.sh` has already been run (web/pkg/ populated).

if [ ! -d "web/pkg" ]; then
    echo "warning: web/pkg/ not found — run 'bash wasm-build.sh' first, or the" >&2
    echo "         deployed app will be missing its WASM module." >&2
fi

rm -rf dist
mkdir -p dist/app

cp -r landing/. dist/
cp -r web/. dist/app/

echo "dist/ assembled:"
echo "  dist/       -> landing page"
echo "  dist/app/   -> player app"
echo "Preview with: python3 -m http.server --directory dist 8080"
