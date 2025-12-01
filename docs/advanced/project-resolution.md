# Project Resolution: How Codanna Understands Your Imports

When you write `import { Button } from '@components/Button'` in TypeScript or JavaScript, Codanna needs to figure out which actual file that refers to. This is called **project resolution** - the process of mapping import paths to real files and symbols.

## The Problem

Consider this TypeScript import:

```typescript
import { UserService } from '@services/user';
```

Without context, `@services/user` is meaningless. It could be:
- `src/services/user.ts`
- `lib/services/user/index.ts`
- Something else entirely

The mapping is defined in your project's configuration files (`tsconfig.json`, `jsconfig.json`, `pom.xml`, `Package.swift`). Codanna reads these files to understand how your project resolves imports.

## How It Works

### 1. Configuration Discovery

When you run `codanna index`, Codanna looks for project configuration files you've specified in `.codanna/settings.toml`:

```toml
[languages.typescript]
config_files = [
    "tsconfig.json",
    "packages/web/tsconfig.json"
]

[languages.javascript]
config_files = [
    "jsconfig.json"
]

[languages.java]
config_files = [
    "pom.xml"
]

[languages.swift]
config_files = [
    "Package.swift"
]
```

### 2. Rule Extraction

Codanna parses each config file and extracts resolution rules.

**TypeScript/JavaScript** (`tsconfig.json` / `jsconfig.json`):
```json
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@components/*": ["src/components/*"],
      "@services/*": ["src/services/*"],
      "@/*": ["src/*"]
    }
  }
}
```

From this, Codanna extracts:
- `@components/Button` → `src/components/Button`
- `@services/user` → `src/services/user`
- `@/utils/helper` → `src/utils/helper`

**Java** (`pom.xml`):
```xml
<project>
  <groupId>com.example</groupId>
  <build>
    <sourceDirectory>src/main/java</sourceDirectory>
  </build>
</project>
```

From this, Codanna knows Java source files live in `src/main/java/`.

### 3. Rule Persistence

Extracted rules are saved to `.codanna/index/resolvers/`:

```
.codanna/index/resolvers/
├── typescript_resolution.json
├── javascript_resolution.json
├── java_resolution.json
└── swift_resolution.json
```

This means Codanna doesn't re-parse config files on every query - it uses the cached rules.

### 4. Import Resolution

When Codanna indexes a file like:

```typescript
// src/pages/Home.tsx
import { Button } from '@components/Button';
import { useAuth } from '@hooks/auth';
```

It resolves each import:
1. `@components/Button` matches pattern `@components/*`
2. Apply replacement: `src/components/Button`
3. Find the `Button` symbol in that module
4. Create a relationship: `Home` uses `Button`

This creates the call graph that powers queries like "who uses Button?"

## Language-Specific Details

### TypeScript / JavaScript

Both languages use the same resolution system based on `tsconfig.json` / `jsconfig.json`.

**Path alias patterns:**
- `@app/*` → wildcard, matches anything after `@app/`
- `@utils` → exact match, no wildcard
- `@/*` → common pattern for "src root"

**Resolution order (most specific wins):**
```json
{
  "paths": {
    "@components/Button": ["src/ui/SpecialButton"],  // Most specific
    "@components/*": ["src/components/*"],            // Less specific
    "@/*": ["src/*"]                                  // Least specific
  }
}
```

For `@components/Button`, the first pattern wins.

**Relative imports:**
```typescript
import { helper } from './utils';      // Same directory
import { config } from '../config';    // Parent directory
```

Codanna resolves these relative to the importing file's location.

### Java

Java resolution uses package names and source directories.

**Package to path mapping:**
```
com.example.service.UserService
    → src/main/java/com/example/service/UserService.java
```

**Multi-module projects:**
```toml
[languages.java]
config_files = [
    "pom.xml",
    "core/pom.xml",
    "api/pom.xml"
]
```

Each module's `pom.xml` defines its source directory, so imports resolve to the correct module.

### Swift

Swift resolution uses Swift Package Manager (SPM) conventions from `Package.swift`.

**Configuration:**
```toml
[languages.swift]
config_files = [
    "Package.swift"
]
```

**Package.swift example:**
```swift
// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "MyApp",
    targets: [
        .target(name: "MyLib"),
        .target(name: "MyApp", dependencies: ["MyLib"]),
        .testTarget(name: "MyLibTests", dependencies: ["MyLib"]),
    ]
)
```

**SPM conventions:**
- `Sources/<ModuleName>/` for library and executable targets
- `Tests/<ModuleName>Tests/` for test targets
- Custom paths via `path:` parameter in target definitions

**Module path mapping:**
```
Sources/MyLib/Types/User.swift -> MyLib.Types
Sources/MyApp/Main.swift -> MyApp
Tests/MyLibTests/UserTests.swift -> MyLibTests
```

**Custom source paths:**
```swift
.target(name: "MyLib", path: "CustomSources/MyLib")
```

Codanna detects custom paths and adjusts resolution accordingly.

## Monorepo Support

For monorepos with multiple `tsconfig.json` files:

```
my-monorepo/
├── tsconfig.json              # Root config
├── packages/
│   ├── web/
│   │   └── tsconfig.json      # Web-specific paths
│   └── api/
│       └── tsconfig.json      # API-specific paths
```

Configure all of them:

```toml
[languages.typescript]
config_files = [
    "tsconfig.json",
    "packages/web/tsconfig.json",
    "packages/api/tsconfig.json"
]
```

Codanna merges rules from all configs, with more specific paths taking precedence.

## Troubleshooting

### "Unresolved import" warnings

If Codanna can't resolve an import, check:

1. **Is the config file listed?**
   ```bash
   cat .codanna/settings.toml | grep config_files
   ```

2. **Does the path pattern match?**
   - `@components/*` matches `@components/Button`
   - `@components/*` does NOT match `@components` (no trailing path)

3. **Is the target file indexed?**
   ```bash
   codanna mcp search_symbols query:Button
   ```

### Checking resolved rules

View what rules Codanna extracted:

```bash
cat .codanna/index/resolvers/typescript_resolution.json
```

### Automatic re-indexing

Codanna tracks SHA-256 hashes of your config files. When you change `tsconfig.json` or `jsconfig.json`, the next `codanna index` automatically detects the change and rebuilds the resolution rules.

You don't need `--force` - just run:

```bash
codanna index .
```

## How Resolution Affects Queries

Good resolution improves query results:

**With resolution:**
```
> codanna retrieve describe LoginForm

LoginForm (Function) at src/pages/Login.tsx
  Uses:
    - Button (Component) at src/components/Button.tsx
    - useAuth (Function) at src/hooks/auth.ts
```

**Without resolution:**
```
> codanna retrieve describe LoginForm

LoginForm (Function) at src/pages/Login.tsx
  Uses:
    - (unresolved: @components/Button)
    - (unresolved: @hooks/auth)
```

The relationships are what make Codanna useful for understanding code dependencies.

## See Also

- [Configuration Guide](../user-guide/configuration.md) - Full settings reference
- [First Index](../getting-started/first-index.md) - Getting started with indexing
