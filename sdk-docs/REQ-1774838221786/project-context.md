test_runner: vitest
test_command: pnpm test
test_file_pattern: "test/**/*.test.ts"
lint_command: pnpm lint:check
typecheck_command: pnpm build
e2e_framework: vitest (integration tests via vitest.integration.config.ts)
e2e_dir: e2e/
e2e_command: pnpm test:integration
component_library: (none - SDK library, not a UI)
state_management: (none)
styling: (none)
framework: typescript
package_manager: pnpm@10.7.0
monorepo_tool: turborepo
repo_type: open-source-sdk
main_packages: "@x402/core, @x402/evm, @x402/svm, @x402/express, @x402/fetch, @x402/axios, @x402/hono, @x402/next, @x402/paywall, @x402/extensions"

# Additional context
node_version: ">=18.0.0"
typescript_version: "^5.8.3"
bundler: tsup
source_dir: typescript/packages/<package>/src
test_dir: typescript/packages/<package>/test
unit_test_dir: test/unit/
integration_test_dir: test/integrations/
build_output: dist/ (CJS + ESM dual output)
changeset_tool: "@changesets/cli"
format_command: pnpm format:check
format_fix_command: pnpm format
working_dir_for_ts: typescript/
per_package_test_command: "cd typescript/packages/core && pnpm test"
per_package_build_command: "cd typescript/packages/core && pnpm build"
