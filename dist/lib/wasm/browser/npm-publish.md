# How to Update Your Package on NPM

Here's the step-by-step process to publish the updated `rq-library-wasm` package:

## 1. **Update the `rq_library.d.ts` file**

Add the filesystem function declarations I provided earlier (lines 15-21).

## 2. **Bump the version in `package.json`**

```bash
# In your rq-library-wasm project directory
npm version patch   # For bug fixes (1.0.1 -> 1.0.2)
# OR
npm version minor   # For new features (1.0.1 -> 1.1.0)
# OR
npm version major   # For breaking changes (1.0.1 -> 2.0.0)
```

Example:

```bash
npm version patch
```

This will:

- Update the version in `package.json` (1.0.1 → 1.0.2)
- Create a git commit with the version change
- Create a git tag (v1.0.2)

### 3. **Publish to NPM**

```bash
npm publish
```

If it's a scoped package or you need to make it public:

```bash
npm publish --access public
```
