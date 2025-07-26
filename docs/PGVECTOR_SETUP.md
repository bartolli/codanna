# pgvector Setup Guide for PostgreSQL 16 on macOS

## The Problem

When installing pgvector via Homebrew, it only builds for specific PostgreSQL versions (typically 14 and 17), but not for PostgreSQL 16. This causes the error:

```
ERROR: extension "vector" is not available
DETAIL: Could not open extension control file "/opt/homebrew/opt/postgresql@16/share/postgresql@16/extension/vector.control": No such file or directory.
```

## The Solution

Build pgvector from source for your specific PostgreSQL version.

## Step-by-Step Instructions

### 1. Prerequisites
- PostgreSQL 16 installed via Homebrew: `brew install postgresql@16`
- Homebrew's PostgreSQL 16 running: `brew services start postgresql@16`

### 2. Build pgvector from Source

```bash
# Clone the pgvector repository
git clone --branch v0.8.0 https://github.com/pgvector/pgvector.git
cd pgvector

# Build for PostgreSQL 16
export PG_CONFIG=/opt/homebrew/opt/postgresql@16/bin/pg_config
make

# Install (no sudo needed for Homebrew installations)
make install
```

### 3. Verify Installation

```sql
-- Connect to PostgreSQL
psql -d postgres

-- Create the extension
CREATE EXTENSION vector;

-- Verify it's installed
SELECT * FROM pg_extension WHERE extname = 'vector';
```

## Common Issues

### Issue 1: Homebrew pgvector Only Supports Certain Versions
**Symptom**: After `brew install pgvector`, you see pgvector files only for PostgreSQL 14 and 17:
```
/opt/homebrew/Cellar/pgvector/0.8.0/lib/postgresql@14/
/opt/homebrew/Cellar/pgvector/0.8.0/lib/postgresql@17/
```

**Solution**: Build from source as shown above.

### Issue 2: Type Conversion Errors in Rust
**Symptom**: When using pgvector with tokio-postgres:
```
error: cannot convert between the Rust type `&[f32]` and the Postgres type `vector`
```

**Solution**: Use the pgvector crate with the postgres feature:
```toml
[dependencies]
pgvector = { version = "0.4", features = ["postgres"] }
```

Then use the `Vector` type:
```rust
use pgvector::Vector;

let embedding = vec![1.0, 2.0, 3.0];
let vector = Vector::from(embedding);
client.execute("INSERT INTO items (embedding) VALUES ($1)", &[&vector])?;
```

## For Future Sessions

1. **Check pgvector compatibility first**:
   ```bash
   brew info pgvector | grep "Build:"
   ```
   This shows which PostgreSQL versions pgvector was built for.

2. **If your PostgreSQL version isn't supported**, build from source immediately instead of trying workarounds.

3. **Test the installation** before proceeding with development:
   ```bash
   psql -c "CREATE EXTENSION IF NOT EXISTS vector" postgres
   ```

## Clean Uninstall/Reinstall Process

If you need to start fresh:
```bash
# Stop PostgreSQL
brew services stop postgresql@16

# Uninstall pgvector
brew uninstall pgvector

# Remove any manually installed files
rm -f /opt/homebrew/opt/postgresql@16/lib/postgresql/vector.*
rm -f /opt/homebrew/opt/postgresql@16/share/postgresql@16/extension/vector.*

# Build from source as shown above
```

## References
- pgvector GitHub: https://github.com/pgvector/pgvector
- pgvector-rust: https://github.com/pgvector/pgvector-rust
- Homebrew pgvector formula: https://formulae.brew.sh/formula/pgvector