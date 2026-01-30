# Validation Standardization Plan

## Philosophy

**NO BACKWARD COMPATIBILITY. NO MIGRATION. NO DEPRECATION.**

We are building the best system possible. If old behavior is broken or incomplete:
- **Replace it directly** with the correct implementation
- **Remove dead code** that serves no purpose
- **Refactor aggressively** to achieve clean architecture
- **No half-measures** - validation either works completely or we fix it completely

This is version 0.1. We implement features cleanly as if building from scratch. No legacy concerns, no compatibility layers, no "TODO: migrate later". Even migration files that need to be edited should be edited instead of introducing a new patch migration.

---

## Current State Analysis

### tool-cli

| Aspect | Current Value | Location |
|--------|---------------|----------|
| First char | lowercase a-z | `lib/validate/validators/fields.rs:170` |
| Remaining | lowercase a-z, 0-9, hyphen | `lib/validate/validators/fields.rs:174` |
| Underscore | **Not allowed** | - |
| Min length | 1 (implicit) | - |
| Max length | **None** | - |
| Duplicates | **3 identical functions** | fields.rs, init.rs, prompt.rs |

### backend-users

| Aspect | Current Value | Location |
|--------|---------------|----------|
| Regex | `^[a-zA-Z][a-zA-Z0-9_-]{2,19}$` | `lib/api/utils/validation.rs:13` |
| First char | letter (a-z, A-Z) | - |
| Remaining | alphanumeric, underscore, hyphen | - |
| Underscore | **Allowed** | - |
| Uppercase | **Allowed** (normalized to lowercase) | - |
| Min length | 3 | - |
| Max length | 20 | - |
| DB column | VARCHAR(100) | `migrations/20250920224112` |

### backend-registries

| Aspect | Current Value | Location |
|--------|---------------|----------|
| Namespace min | 3 | `lib/defaults.rs:83` |
| Namespace max | 100 | `lib/defaults.rs:84` |
| Namespace chars | alphanumeric, hyphen, underscore | `lib/defaults.rs:128` |
| Artifact min | 1 | `lib/defaults.rs:85` |
| Artifact max | 255 | `lib/defaults.rs:86` |
| Artifact chars | **Any** (only no leading/trailing hyphen) | `lib/defaults.rs:131-135` |

---

## Problems

1. **Inconsistent rules**: tool-cli is strict, backend-registries is permissive, backend-users is in between
2. **Uppercase allowed** in backend-users but not tool-cli
3. **Underscores allowed** in backend-users and backend-registries but not tool-cli
4. **No max length** in tool-cli
5. **Artifact names too permissive** in backend-registries (allows almost anything)
6. **Duplicate code** in tool-cli (3 identical validation functions)
7. **OAuth incompatibility**: GitHub doesn't allow underscores, Google doesn't allow hyphens

---

## Proposed Standard

### Universal Rule

**One pattern for everything:**

```
Pattern: ^[a-z][a-z0-9-]{2,63}$
```

| Rule | Value | Rationale |
|------|-------|-----------|
| First char | lowercase letter (a-z) | URL-safe, DNS-safe, prevents numeric conflicts |
| Remaining | lowercase letters, digits, hyphens | GitHub-compatible (no underscores), URL-friendly |
| Min length | 3 | Prevents squatting on short names |
| Max length | 64 | DNS label max (63 + null), power of 2, practical limit |
| Uppercase | Reject (not just normalize) | Simplicity, no case confusion |
| Underscores | **Not allowed** | GitHub incompatible, URL-unfriendly |

This applies to:
- Usernames
- Organization slugs
- Namespaces
- Package/artifact names

### OAuth Username Handling

When users login via GitHub/Google:
1. Fetch their username from provider
2. Normalize: lowercase, replace non-alphanumeric with hyphens, collapse consecutive hyphens, trim hyphens
3. If result is valid, use it
4. If result is invalid or taken, prompt user to choose a username

```rust
fn normalize_oauth_username(external: &str) -> String {
    let normalized: String = external
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in normalized.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}
```

---

## Implementation Plan

### Phase 1: backend-users

#### 1.1 Update validation.rs

**File**: `services/backend-users/lib/api/utils/validation.rs`

**Delete** lines 13-14 (old regex):
```rust
pub static ref USERNAME_REGEX: Regex = Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]{2,19}$").unwrap();
```

**Replace with**:
```rust
pub static ref USERNAME_REGEX: Regex = Regex::new(r"^[a-z][a-z0-9-]{2,63}$").unwrap();
```

**Update** `validate_username()` (lines 49-57):
```rust
/// Validates username format (3-64 chars, lowercase, starts with letter, no underscores)
pub fn validate_username(username: &str) -> Result<(), ValidationError> {
    if !USERNAME_REGEX.is_match(username) {
        let mut error = ValidationError::new("invalid_username");
        error.message = Some("Username must be 3-64 characters, start with a lowercase letter, and contain only lowercase letters, numbers, and hyphens".into());
        return Err(error);
    }
    Ok(())
}
```

**Update** `validate_slug()` (lines 59-67) - same pattern:
```rust
/// Validates slug format (3-64 chars, lowercase, starts with letter, no underscores)
pub fn validate_slug(slug: &str) -> Result<(), ValidationError> {
    if !USERNAME_REGEX.is_match(slug) {
        let mut error = ValidationError::new("invalid_slug");
        error.message = Some("Slug must be 3-64 characters, start with a lowercase letter, and contain only lowercase letters, numbers, and hyphens".into());
        return Err(error);
    }
    Ok(())
}
```

**Update tests** (lines 280-290):
```rust
#[test]
fn test_username_validation() {
    // Valid
    assert!(validate_username("user123").is_ok());
    assert!(validate_username("user-name").is_ok());
    assert!(validate_username("abc").is_ok());
    assert!(validate_username(&"a".repeat(64)).is_ok());

    // Invalid - too short
    assert!(validate_username("ab").is_err());

    // Invalid - too long
    assert!(validate_username(&"a".repeat(65)).is_err());

    // Invalid - uppercase
    assert!(validate_username("User123").is_err());
    assert!(validate_username("USER").is_err());

    // Invalid - underscore (no longer allowed)
    assert!(validate_username("user_name").is_err());

    // Invalid - starts with number
    assert!(validate_username("123user").is_err());

    // Invalid - starts with hyphen
    assert!(validate_username("-user").is_err());

    // Invalid - special characters
    assert!(validate_username("user@123").is_err());
    assert!(validate_username("user.name").is_err());
}
```

#### 1.2 Update auth.rs types

**File**: `services/backend-users/lib/api/types/auth.rs`

**Update** `RegisterRequest` (line 15):
```rust
#[validate(length(min = 3, max = 64, message = "Username must be 3-64 characters"), custom(function = "crate::api::utils::validation::validate_username"))]
pub username: String,
```

#### 1.3 Update organization.rs types

**File**: `services/backend-users/lib/api/types/organization.rs`

**Update** `CreateOrganizationRequest` (line 32):
```rust
#[validate(length(min = 3, max = 64, message = "Slug must be 3-64 characters"), custom(function = "crate::api::utils::validation::validate_slug"))]
pub slug: String,
```

#### 1.4 Update auth handler

**File**: `services/backend-users/lib/api/handlers/auth.rs`

**Update** normalization (lines 74-76) to reject uppercase instead of normalizing:
```rust
// Normalize email only - username must already be lowercase
let email = request.email.trim().to_lowercase();
let username = request.username.trim();

// Reject if username contains uppercase (validation will catch format, this is for clear error)
if username.chars().any(|c| c.is_ascii_uppercase()) {
    return Err(ApiError::validation("Username must be lowercase"));
}
```

#### 1.5 Update database migration

**File**: `services/backend-users/migrations/20250920224112_create_users_table.up.sql`

**Change** line 4:
```sql
username VARCHAR(64) NOT NULL UNIQUE,
```

**File**: `services/backend-users/migrations/20250920224117_create_organizations_table.up.sql`

**Change** line 4:
```sql
slug VARCHAR(64) NOT NULL UNIQUE,
```

#### 1.6 Add OAuth username normalization

**File**: `services/backend-users/lib/api/utils/validation.rs`

**Add** new function:
```rust
/// Normalize external OAuth username to our format
pub fn normalize_oauth_username(external: &str) -> String {
    let normalized: String = external
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in normalized.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}

/// Check if normalized username is valid
pub fn is_valid_normalized_username(username: &str) -> bool {
    USERNAME_REGEX.is_match(username)
}
```

---

### Phase 2: backend-registries

#### 2.1 Update defaults.rs constants

**File**: `services/backend-registries/lib/defaults.rs`

**Update** constants (lines 83-86):
```rust
pub const MIN_NAME_LENGTH: usize = 3;
pub const MAX_NAME_LENGTH: usize = 64;
```

#### 2.2 Update validation functions

**File**: `services/backend-registries/lib/defaults.rs`

**Replace** `is_valid_namespace()` (lines 125-129):
```rust
/// Validate namespace format (3-64 chars, lowercase, starts with letter, no underscores)
pub fn is_valid_namespace(namespace: &str) -> bool {
    let len = namespace.len();
    if len < MIN_NAME_LENGTH || len > MAX_NAME_LENGTH {
        return false;
    }
    let mut chars = namespace.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
```

**Replace** `is_valid_artifact_name()` (lines 131-136):
```rust
/// Validate artifact name format (3-64 chars, lowercase, starts with letter, no underscores)
pub fn is_valid_artifact_name(name: &str) -> bool {
    let len = name.len();
    if len < MIN_NAME_LENGTH || len > MAX_NAME_LENGTH {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
```

**Update** `default_slug()` (lines 103-110):
```rust
/// Generate URL-friendly slug from name
pub fn default_slug(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}
```

#### 2.3 Update tests

**File**: `services/backend-registries/lib/defaults.rs`

**Replace** tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_slug() {
        assert_eq!(default_slug("My Cool Tool"), "my-cool-tool");
        assert_eq!(default_slug("tool_name"), "tool-name");
        assert_eq!(default_slug("UPPERCASE"), "uppercase");
        assert_eq!(default_slug("--double--hyphen--"), "double-hyphen");
        assert_eq!(default_slug("special@#$chars"), "special-chars");
        assert_eq!(default_slug("  spaces  "), "spaces");
    }

    #[test]
    fn test_is_valid_namespace() {
        // Valid
        assert!(is_valid_namespace("abc"));
        assert!(is_valid_namespace("my-namespace"));
        assert!(is_valid_namespace("user123"));
        assert!(is_valid_namespace(&"a".repeat(64)));

        // Invalid - too short
        assert!(!is_valid_namespace("ab"));

        // Invalid - too long
        assert!(!is_valid_namespace(&"a".repeat(65)));

        // Invalid - uppercase
        assert!(!is_valid_namespace("MyNamespace"));

        // Invalid - underscore
        assert!(!is_valid_namespace("my_namespace"));

        // Invalid - starts with number
        assert!(!is_valid_namespace("123namespace"));

        // Invalid - starts with hyphen
        assert!(!is_valid_namespace("-namespace"));
    }

    #[test]
    fn test_is_valid_artifact_name() {
        // Valid
        assert!(is_valid_artifact_name("abc"));
        assert!(is_valid_artifact_name("my-tool"));
        assert!(is_valid_artifact_name("tool123"));
        assert!(is_valid_artifact_name(&"a".repeat(64)));

        // Invalid - too short
        assert!(!is_valid_artifact_name("ab"));

        // Invalid - too long
        assert!(!is_valid_artifact_name(&"a".repeat(65)));

        // Invalid - uppercase
        assert!(!is_valid_artifact_name("MyTool"));

        // Invalid - underscore
        assert!(!is_valid_artifact_name("my_tool"));

        // Invalid - starts with number
        assert!(!is_valid_artifact_name("123tool"));

        // Invalid - starts with hyphen
        assert!(!is_valid_artifact_name("-tool"));
    }

    #[test]
    fn test_is_valid_version() {
        assert!(is_valid_version("1.0.0"));
        assert!(is_valid_version("0.1.0-alpha"));
        assert!(is_valid_version("2.0.0-rc.1+build.123"));
        assert!(!is_valid_version("1.0"));
        assert!(!is_valid_version("v1.0.0"));
        assert!(!is_valid_version("01.0.0"));
    }

    #[test]
    fn test_is_valid_visibility() {
        assert!(is_valid_visibility("public"));
        assert!(is_valid_visibility("private"));
        assert!(is_valid_visibility("unlisted"));
        assert!(!is_valid_visibility("secret"));
        assert!(!is_valid_visibility(""));
    }
}
```

#### 2.4 Update database migration

**File**: `services/backend-registries/migrations/20250918093610_create_artifacts_table.up.sql`

**Update** column definitions:
```sql
namespace VARCHAR(64) NOT NULL,
name VARCHAR(64) NOT NULL,
slug VARCHAR(64) NOT NULL,
```

---

### Phase 3: tool-cli

#### 3.1 Create single source of truth

**File**: `tool-cli/lib/validate/validators/fields.rs`

**Update** `is_valid_package_name()` (lines 164-175):
```rust
/// Minimum package name length
pub const MIN_PACKAGE_NAME_LENGTH: usize = 3;

/// Maximum package name length
pub const MAX_PACKAGE_NAME_LENGTH: usize = 64;

/// Check if a package name is valid.
/// Rules: 3-64 chars, starts with lowercase letter, contains only lowercase letters, digits, hyphens
pub fn is_valid_package_name(name: &str) -> bool {
    let len = name.len();
    if len < MIN_PACKAGE_NAME_LENGTH || len > MAX_PACKAGE_NAME_LENGTH {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
```

**Update** error message in `validate_formats()` (lines 76-87):
```rust
// Validate name format
if let Some(name) = &manifest.name
    && !is_valid_package_name(name)
{
    result.errors.push(ValidationIssue {
        code: ErrorCode::InvalidPackageName.into(),
        message: "invalid package name".into(),
        location: "manifest.json:name".into(),
        details: format!("`{}` must be 3-64 lowercase alphanumeric chars with hyphens, starting with a letter", name),
        help: Some("use format: my-package-name (3-64 chars)".into()),
    });
}
```

#### 3.2 Remove duplicate in init.rs

**File**: `tool-cli/lib/handlers/tool/init.rs`

**Delete** lines 758-769 (the duplicate `is_valid_package_name` function)

**Add import** at top of file:
```rust
use crate::validate::validators::fields::is_valid_package_name;
```

**Update** error message (lines 162-168):
```rust
// Validate name format
if !is_valid_package_name(&pkg_name) {
    return Err(ToolError::Generic(format!(
        "Invalid package name \"{}\"\nName must be 3-64 characters, start with a lowercase letter, and contain only lowercase letters, numbers, and hyphens.",
        pkg_name
    )));
}
```

#### 3.3 Remove duplicate in prompt.rs

**File**: `tool-cli/lib/prompt.rs`

**Delete** lines 183-199 (the duplicate `is_valid_package_name` function)

**Add import** at top of file:
```rust
use crate::validate::validators::fields::is_valid_package_name;
```

**Update** validation message (lines 221-228):
```rust
.validate(|input: &String| {
    if input.is_empty() {
        Err("Package name is required")
    } else if !is_valid_package_name(input) {
        Err("Must be 3-64 lowercase letters, numbers, and hyphens, starting with a letter")
    } else {
        Ok(())
    }
})
```

#### 3.4 Update tests

**File**: `tool-cli/lib/validate/tests.rs`

**Delete** lines 7-17 (duplicate function)

**Add import**:
```rust
use crate::validate::validators::fields::is_valid_package_name;
```

**Update** test (lines 19-29):
```rust
#[test]
fn test_valid_package_name() {
    // Valid
    assert!(is_valid_package_name("my-tool"));
    assert!(is_valid_package_name("tool123"));
    assert!(is_valid_package_name("abc"));
    assert!(is_valid_package_name(&"a".repeat(64)));

    // Invalid - too short
    assert!(!is_valid_package_name("ab"));
    assert!(!is_valid_package_name("a"));

    // Invalid - too long
    assert!(!is_valid_package_name(&"a".repeat(65)));

    // Invalid - empty
    assert!(!is_valid_package_name(""));

    // Invalid - uppercase
    assert!(!is_valid_package_name("My-Tool"));
    assert!(!is_valid_package_name("TOOL"));

    // Invalid - starts with digit
    assert!(!is_valid_package_name("123tool"));

    // Invalid - starts with hyphen
    assert!(!is_valid_package_name("-tool"));

    // Invalid - underscore
    assert!(!is_valid_package_name("tool_name"));

    // Invalid - special chars
    assert!(!is_valid_package_name("tool@name"));
    assert!(!is_valid_package_name("tool.name"));
}
```

#### 3.5 Export from lib.rs

**File**: `tool-cli/lib/lib.rs`

Ensure `is_valid_package_name` is exported if needed by other crates.

#### 3.6 Update references.rs (PluginRef parser)

**File**: `tool-cli/lib/references.rs`

The `PluginRef` struct is used for parsing tool references like `namespace/tool-name@version`.
This validation MUST match the unified standard.

**Update** constants (lines 17-21):
```rust
/// Regex pattern for validating namespace segments.
/// Rules: 3-64 chars, starts with lowercase letter, contains only lowercase letters, digits, hyphens
const NAMESPACE_PATTERN: &str = r"^[a-z][a-z0-9-]{2,63}$";

/// Regex pattern for validating name segments.
/// Rules: 3-64 chars, starts with lowercase letter, contains only lowercase letters, digits, hyphens
const NAME_PATTERN: &str = r"^[a-z][a-z0-9-]{2,63}$";
```

**Update** `validate_namespace()` (lines 183-202):
```rust
fn validate_namespace(namespace: &str) -> ToolResult<()> {
    if namespace.len() < 3 {
        return Err(ToolError::InvalidReference(format!(
            "Namespace '{}' must be at least 3 characters",
            namespace
        )));
    }
    if namespace.len() > 64 {
        return Err(ToolError::InvalidReference(format!(
            "Namespace '{}' exceeds 64 character limit",
            namespace
        )));
    }
    if !NAMESPACE_REGEX.is_match(namespace) {
        return Err(ToolError::InvalidReference(format!(
            "Namespace '{}' must start with lowercase letter and contain only lowercase letters, numbers, and hyphens",
            namespace
        )));
    }
    Ok(())
}
```

**Update** `validate_name()` (lines 205-223):
```rust
fn validate_name(name: &str) -> ToolResult<()> {
    if name.len() < 3 {
        return Err(ToolError::InvalidReference(format!(
            "Name '{}' must be at least 3 characters",
            name
        )));
    }
    if name.len() > 64 {
        return Err(ToolError::InvalidReference(format!(
            "Name '{}' exceeds 64 character limit",
            name
        )));
    }
    if !NAME_REGEX.is_match(name) {
        return Err(ToolError::InvalidReference(format!(
            "Name '{}' must start with lowercase letter and contain only lowercase letters, numbers, and hyphens",
            name
        )));
    }
    Ok(())
}
```

**Key changes:**
- No underscores allowed (removed `_` from pattern)
- Minimum length: 3 chars (was 2 for namespace, 1 for name)
- Maximum length: 64 chars (was 50 for namespace, 100 for name)
- Error messages updated to remove mention of underscores

---

### Phase 4: frontend-plugin.store

#### 4.1 Create validation schema

**File**: `services/frontend-plugin.store/lib/validations/auth.ts` (new file)

```typescript
import { z } from "zod";

// Shared validation pattern - matches backend exactly
const usernamePattern = /^[a-z][a-z0-9-]{2,63}$/;

export const registerSchema = z
  .object({
    username: z
      .string()
      .min(3, "Username must be at least 3 characters")
      .max(64, "Username must be at most 64 characters")
      .regex(
        usernamePattern,
        "Username must be lowercase letters, numbers, and hyphens, starting with a letter"
      ),
    email: z.string().email("Invalid email address"),
    password: z
      .string()
      .min(8, "Password must be at least 8 characters")
      .max(128, "Password must be at most 128 characters"),
    confirmPassword: z.string(),
    terms: z.literal(true, {
      errorMap: () => ({ message: "You must accept the terms and conditions" }),
    }),
  })
  .refine((data) => data.password === data.confirmPassword, {
    message: "Passwords don't match",
    path: ["confirmPassword"],
  });

export type RegisterFormData = z.infer<typeof registerSchema>;

export const loginSchema = z.object({
  email: z.string().email("Invalid email address"),
  password: z.string().min(1, "Password is required"),
});

export type LoginFormData = z.infer<typeof loginSchema>;
```

#### 4.2 Update register page

**File**: `services/frontend-plugin.store/app/register/page.tsx`

**Replace** the current useState-based form with React Hook Form + Zod:

```typescript
"use client";

import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { registerSchema, RegisterFormData } from "@/lib/validations/auth";
import { apiClient } from "@/lib/api/client";
import { useState } from "react";
import { toast } from "sonner";
import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

export default function RegisterPage() {
  const [isLoading, setIsLoading] = useState(false);
  const [isSuccess, setIsSuccess] = useState(false);

  const {
    register,
    handleSubmit,
    setValue,
    watch,
    formState: { errors },
  } = useForm<RegisterFormData>({
    resolver: zodResolver(registerSchema),
    defaultValues: {
      username: "",
      email: "",
      password: "",
      confirmPassword: "",
      terms: false,
    },
  });

  const onSubmit = async (data: RegisterFormData) => {
    setIsLoading(true);
    try {
      await apiClient.register({
        username: data.username,
        email: data.email,
        password: data.password,
      });
      setIsSuccess(true);
      toast.success("Registration successful! Please check your email.");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Registration failed");
    } finally {
      setIsLoading(false);
    }
  };

  if (isSuccess) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <Card className="w-full max-w-md">
          <CardHeader>
            <CardTitle>Check your email</CardTitle>
            <CardDescription>
              We sent you a verification link. Please check your email to complete registration.
            </CardDescription>
          </CardHeader>
          <CardFooter>
            <Link href="/login" className="w-full">
              <Button variant="outline" className="w-full">
                Back to login
              </Button>
            </Link>
          </CardFooter>
        </Card>
      </div>
    );
  }

  return (
    <div className="flex min-h-screen items-center justify-center">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>Create an account</CardTitle>
          <CardDescription>Enter your details to get started</CardDescription>
        </CardHeader>
        <form onSubmit={handleSubmit(onSubmit)}>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                placeholder="my-username"
                {...register("username")}
              />
              {errors.username && (
                <p className="text-sm text-destructive">{errors.username.message}</p>
              )}
            </div>
            <div className="space-y-2">
              <Label htmlFor="email">Email</Label>
              <Input
                id="email"
                type="email"
                placeholder="you@example.com"
                {...register("email")}
              />
              {errors.email && (
                <p className="text-sm text-destructive">{errors.email.message}</p>
              )}
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                {...register("password")}
              />
              {errors.password && (
                <p className="text-sm text-destructive">{errors.password.message}</p>
              )}
            </div>
            <div className="space-y-2">
              <Label htmlFor="confirmPassword">Confirm Password</Label>
              <Input
                id="confirmPassword"
                type="password"
                {...register("confirmPassword")}
              />
              {errors.confirmPassword && (
                <p className="text-sm text-destructive">{errors.confirmPassword.message}</p>
              )}
            </div>
            <div className="flex items-center space-x-2">
              <Checkbox
                id="terms"
                checked={watch("terms")}
                onCheckedChange={(checked) => setValue("terms", checked === true)}
              />
              <Label htmlFor="terms" className="text-sm">
                I agree to the{" "}
                <Link href="/terms" className="underline">
                  terms and conditions
                </Link>
              </Label>
            </div>
            {errors.terms && (
              <p className="text-sm text-destructive">{errors.terms.message}</p>
            )}
          </CardContent>
          <CardFooter className="flex flex-col space-y-4">
            <Button type="submit" className="w-full" disabled={isLoading}>
              {isLoading ? "Creating account..." : "Create account"}
            </Button>
            <p className="text-sm text-muted-foreground">
              Already have an account?{" "}
              <Link href="/login" className="underline">
                Sign in
              </Link>
            </p>
          </CardFooter>
        </form>
      </Card>
    </div>
  );
}
```

#### 4.3 Update login page (optional but recommended)

**File**: `services/frontend-plugin.store/app/login/page.tsx`

Add Zod validation for consistency (uses `loginSchema` from the same file).

#### 4.4 Add tests for validation schemas

**File**: `services/frontend-plugin.store/lib/validations/auth.test.ts` (new file)

```typescript
import { describe, it, expect } from "vitest";
import { registerSchema, loginSchema } from "./auth";

describe("registerSchema", () => {
  describe("username validation", () => {
    it("accepts valid usernames", () => {
      const validUsernames = [
        "abc",
        "my-username",
        "user123",
        "a".repeat(64),
      ];

      for (const username of validUsernames) {
        const result = registerSchema.safeParse({
          username,
          email: "test@example.com",
          password: "Password123",
          confirmPassword: "Password123",
          terms: true,
        });
        expect(result.success, `Expected "${username}" to be valid`).toBe(true);
      }
    });

    it("rejects usernames that are too short", () => {
      const result = registerSchema.safeParse({
        username: "ab",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("at least 3");
    });

    it("rejects usernames that are too long", () => {
      const result = registerSchema.safeParse({
        username: "a".repeat(65),
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("at most 64");
    });

    it("rejects uppercase usernames", () => {
      const result = registerSchema.safeParse({
        username: "MyUsername",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("lowercase");
    });

    it("rejects usernames with underscores", () => {
      const result = registerSchema.safeParse({
        username: "my_username",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
    });

    it("rejects usernames starting with a number", () => {
      const result = registerSchema.safeParse({
        username: "123user",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
    });

    it("rejects usernames starting with a hyphen", () => {
      const result = registerSchema.safeParse({
        username: "-username",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
    });

    it("rejects usernames with special characters", () => {
      const invalidUsernames = ["user@name", "user.name", "user!name"];

      for (const username of invalidUsernames) {
        const result = registerSchema.safeParse({
          username,
          email: "test@example.com",
          password: "Password123",
          confirmPassword: "Password123",
          terms: true,
        });
        expect(result.success, `Expected "${username}" to be invalid`).toBe(false);
      }
    });
  });

  describe("password validation", () => {
    it("rejects passwords that are too short", () => {
      const result = registerSchema.safeParse({
        username: "validuser",
        email: "test@example.com",
        password: "Short1",
        confirmPassword: "Short1",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("at least 8");
    });

    it("rejects mismatched passwords", () => {
      const result = registerSchema.safeParse({
        username: "validuser",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Different123",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("match");
    });
  });

  describe("email validation", () => {
    it("rejects invalid emails", () => {
      const result = registerSchema.safeParse({
        username: "validuser",
        email: "not-an-email",
        password: "Password123",
        confirmPassword: "Password123",
        terms: true,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("email");
    });
  });

  describe("terms validation", () => {
    it("rejects when terms not accepted", () => {
      const result = registerSchema.safeParse({
        username: "validuser",
        email: "test@example.com",
        password: "Password123",
        confirmPassword: "Password123",
        terms: false,
      });
      expect(result.success).toBe(false);
      expect(result.error?.issues[0].message).toContain("terms");
    });
  });
});

describe("loginSchema", () => {
  it("accepts valid login credentials", () => {
    const result = loginSchema.safeParse({
      email: "test@example.com",
      password: "anypassword",
    });
    expect(result.success).toBe(true);
  });

  it("rejects invalid email", () => {
    const result = loginSchema.safeParse({
      email: "not-an-email",
      password: "anypassword",
    });
    expect(result.success).toBe(false);
  });

  it("rejects empty password", () => {
    const result = loginSchema.safeParse({
      email: "test@example.com",
      password: "",
    });
    expect(result.success).toBe(false);
  });
});
```

---

## Summary of Changes

### Files to Modify

| Codebase | File | Changes |
|----------|------|---------|
| backend-users | `lib/api/utils/validation.rs` | New regex, update functions, add OAuth helper |
| backend-users | `lib/api/types/auth.rs` | Update length validation |
| backend-users | `lib/api/types/organization.rs` | Update length validation |
| backend-users | `lib/api/handlers/auth.rs` | Reject uppercase instead of normalize |
| backend-users | `migrations/20250920224112_create_users_table.up.sql` | VARCHAR(64) |
| backend-users | `migrations/20250920224117_create_organizations_table.up.sql` | VARCHAR(64) |
| backend-registries | `lib/defaults.rs` | Update constants, validation functions, tests |
| backend-registries | `migrations/20250918093610_create_artifacts_table.up.sql` | Update column sizes |
| tool-cli | `lib/validate/validators/fields.rs` | Add constants, update function and error |
| tool-cli | `lib/handlers/tool/init.rs` | Remove duplicate, use import |
| tool-cli | `lib/prompt.rs` | Remove duplicate, use import |
| tool-cli | `lib/validate/tests.rs` | Remove duplicate, update tests |
| tool-cli | `lib/references.rs` | Update regex patterns (3-64 chars, no underscores) |
| frontend-plugin.store | `lib/validations/auth.ts` | New file - Zod schemas |
| frontend-plugin.store | `lib/validations/auth.test.ts` | New file - validation tests |
| frontend-plugin.store | `app/register/page.tsx` | Refactor to React Hook Form + Zod |
| frontend-plugin.store | `app/login/page.tsx` | Add Zod validation (optional) |

### Validation Rules Summary

**One rule for everything:**

| Type | Pattern | Length | Example |
|------|---------|--------|---------|
| Username | `^[a-z][a-z0-9-]{2,63}$` | 3-64 | `john-doe` |
| Namespace | `^[a-z][a-z0-9-]{2,63}$` | 3-64 | `acme-corp` |
| Org Slug | `^[a-z][a-z0-9-]{2,63}$` | 3-64 | `my-org` |
| Package Name | `^[a-z][a-z0-9-]{2,63}$` | 3-64 | `my-awesome-tool` |
| Artifact Name | `^[a-z][a-z0-9-]{2,63}$` | 3-64 | `data-fetcher` |

### What's NOT Allowed

- Uppercase letters (must be lowercase)
- Underscores (use hyphens instead)
- Starting with a number
- Starting or ending with a hyphen
- Special characters (only alphanumeric + hyphen)
- Names shorter than 3 characters
- Names longer than 64 characters
