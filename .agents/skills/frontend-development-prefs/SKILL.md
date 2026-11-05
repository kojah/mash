---
name: frontend-development-prefs
description: Use when starting a frontend project or choosing its framework, package manager, styling, tests, formatting, linting, components, or icons.
---

# Frontend Development Preferences

Build new frontends with the preferred stack unless explicit requirements or
an established repository require otherwise.

## Preferred stack

- TypeScript 7 native compiler with strict checking
- Svelte 5
- Vite
- Vitest
- Tailwind CSS 4
- pnpm
- Biome for formatting and linting
- shadcn-svelte for reusable UI components
- Lucide Svelte for interface icons

Follow the repository's TypeScript conventions for TypeScript work.

For Svelte, run `svelte-check --tsgo`. If current `svelte-check` requires
TypeScript 6 alongside 7, keep 6 only as its compatibility dependency and
install 7 through `@typescript/native@npm:typescript@7`. Do not downgrade the
project checker; verify it executes TypeScript 7.

## Boundaries

Inspect repository instructions, manifests, lockfiles, and existing tooling
before changing anything. Preserve an established stack unless the user asks
for migration. Explicit user and repository requirements override these
defaults.

For new projects, do not substitute npm or Yarn, React or Next.js, ESLint or
Prettier, another component kit, or hand-drawn interface SVGs without a
constraint requiring the substitution. Report any compatibility issue that
prevents the preferred choice.

Install dependencies with pnpm. Use package scripts for development, checking,
testing, and building. Keep generated configuration minimal.

## Verification

Before completion, run Biome checks, TypeScript checking, Vitest, and the
production build. Report unavailable or failing gates.
