// Package main provides security operations for WASM sandboxing and path validation
// Implements enhanced security features for the file operations component
package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// resolvePath returns the absolute, symlink-resolved form of p. Trailing
// components that do not exist yet are appended unresolved to the deepest
// existing ancestor, so a destination that is about to be created can be
// checked. A dangling symlink on the way is an error: following it later
// could land anywhere.
func resolvePath(p string) (string, error) {
	abs, err := filepath.Abs(p)
	if err != nil {
		return "", err
	}
	existing := abs
	var rest []string
	for {
		resolved, err := filepath.EvalSymlinks(existing)
		if err == nil {
			return filepath.Join(append([]string{resolved}, rest...)...), nil
		}
		if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
		if _, lerr := os.Lstat(existing); lerr == nil { // #nosec G703 -- Lstat is part of the containment check
			return "", fmt.Errorf("dangling symlink: %s", existing)
		}
		parent := filepath.Dir(existing)
		if parent == existing {
			return abs, nil
		}
		rest = append([]string{filepath.Base(existing)}, rest...)
		existing = parent
	}
}

// isWithin reports whether p resolves to root or to a path below it. Both
// sides are resolved with resolvePath and compared with filepath.Rel, so
// "/work-other" is not inside "/work" and a symlink out of root is caught.
func isWithin(root, p string) bool {
	r, err := resolvePath(root)
	if err != nil {
		return false
	}
	q, err := resolvePath(p)
	if err != nil {
		return false
	}
	rel, err := filepath.Rel(r, q)
	if err != nil || filepath.IsAbs(rel) {
		return false
	}
	return rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

// requireWithin returns an error unless p resolves inside root.
func requireWithin(root, p, what string) error {
	if !isWithin(root, p) {
		return fmt.Errorf("%s %s resolves outside %s", what, p, root)
	}
	return nil
}

// isWithinAny reports whether p resolves inside at least one of roots.
func isWithinAny(roots []string, p string) bool {
	for _, root := range roots {
		if isWithin(root, p) {
			return true
		}
	}
	return false
}

// SecurityLevel represents different levels of security enforcement
type SecurityLevel int

const (
	SecurityStandard SecurityLevel = iota
	SecurityHigh
	SecurityStrict
)

// SecurityContext provides information about the current security configuration
type SecurityContext struct {
	Level          SecurityLevel `json:"level"`
	AccessibleDirs []string      `json:"accessible_dirs"`
	Restrictions   []string      `json:"restrictions"`
}

// SecurityConfig represents security configuration for operations
type SecurityConfig struct {
	Level             SecurityLevel `json:"level"`
	AllowedDirs       []string      `json:"allowed_dirs"`
	DeniedPatterns    []string      `json:"denied_patterns"`
	EnforceValidation bool          `json:"enforce_validation"`
}

// PreopenDirConfig represents configuration for WASI preopen directories
type PreopenDirConfig struct {
	HostPath    string            `json:"host_path"`
	VirtualPath string            `json:"virtual_path"`
	Permissions AccessPermissions `json:"permissions"`
}

// AccessPermissions represents access permissions for preopen directories
type AccessPermissions int

const (
	AccessReadOnly AccessPermissions = iota
	AccessReadWrite
	AccessFull
)

// Global security context (would be configured by WASI runtime)
var currentSecurityContext = SecurityContext{
	Level:          SecurityStandard,
	AccessibleDirs: []string{},
	Restrictions:   []string{},
}

// ValidatePath validates a path against security policies
// Implements the validate-path WIT interface function
func ValidatePath(path string, allowedDirs []string) error {
	// Always check for path traversal
	if containsPathTraversal(path) {
		return fmt.Errorf("path contains path traversal attempts: %s", path)
	}

	// Apply security level specific validations
	switch currentSecurityContext.Level {
	case SecurityStandard:
		return validatePathStandard(path, allowedDirs)
	case SecurityHigh:
		return validatePathHigh(path, allowedDirs)
	case SecurityStrict:
		return validatePathStrict(path, allowedDirs)
	default:
		return fmt.Errorf("unknown security level")
	}
}

// ConfigurePreopenDirs configures preopen directories for WASI sandboxing
// Implements the configure-preopen-dirs WIT interface function
func ConfigurePreopenDirs(configs []PreopenDirConfig) error {
	// In a real WASI environment, this would configure the runtime
	// For now, we update our security context

	var accessibleDirs []string
	var restrictions []string

	for _, config := range configs {
		accessibleDirs = append(accessibleDirs, config.VirtualPath)

		switch config.Permissions {
		case AccessReadOnly:
			restrictions = append(restrictions, fmt.Sprintf("%s: read-only", config.VirtualPath))
		case AccessReadWrite:
			restrictions = append(restrictions, fmt.Sprintf("%s: read-write", config.VirtualPath))
		case AccessFull:
			restrictions = append(restrictions, fmt.Sprintf("%s: full access", config.VirtualPath))
		}
	}

	currentSecurityContext.AccessibleDirs = accessibleDirs
	currentSecurityContext.Restrictions = restrictions

	return nil
}

// ValidateOperation validates an operation against security policy
// Implements the validate-operation WIT interface function
func ValidateOperation(operation string, paths []string) error {
	// Validate all paths in the operation
	for _, path := range paths {
		if err := ValidatePath(path, currentSecurityContext.AccessibleDirs); err != nil {
			return fmt.Errorf("operation %s failed path validation: %w", operation, err)
		}
	}

	// Operation-specific validations
	switch operation {
	case "copy_file", "copy_directory":
		return validateCopyOperation(paths)
	case "create_directory":
		return validateCreateOperation(paths)
	case "remove_path":
		return validateRemoveOperation(paths)
	case "run_command":
		return validateCommandOperation(paths)
	default:
		return fmt.Errorf("unknown operation: %s", operation)
	}
}

// GetSecurityContext returns current security context information
// Implements the get-security-context WIT interface function
func GetSecurityContext() SecurityContext {
	return currentSecurityContext
}

// Security validation helpers

// validatePathStandard performs standard security validation
func validatePathStandard(path string, allowedDirs []string) error {
	// Basic path traversal check already done. Explicit allowed directories
	// are enforced at every level; the JSON bridge and workspace preparation
	// add the workspace bound themselves.
	if len(allowedDirs) > 0 && !isWithinAny(allowedDirs, path) {
		return fmt.Errorf("path %s not within allowed directories", path)
	}
	return nil
}

// validatePathHigh performs high security validation
func validatePathHigh(path string, allowedDirs []string) error {
	if len(allowedDirs) > 0 && !isWithinAny(allowedDirs, path) {
		return fmt.Errorf("path %s not within allowed directories", path)
	}

	if len(currentSecurityContext.AccessibleDirs) > 0 &&
		!isWithinAny(currentSecurityContext.AccessibleDirs, path) {
		return fmt.Errorf("path %s not accessible in current security context", path)
	}

	return nil
}

// validatePathStrict performs strict security validation
func validatePathStrict(path string, allowedDirs []string) error {
	// All high security checks
	if err := validatePathHigh(path, allowedDirs); err != nil {
		return err
	}

	// Additional strict checks
	absPath, err := filepath.Abs(path)
	if err != nil {
		return fmt.Errorf("cannot resolve absolute path: %w", err)
	}

	// Strict mode requires explicit allow-listing
	if len(allowedDirs) == 0 {
		return fmt.Errorf("strict security mode requires explicit allowed directories")
	}

	// Check for suspicious patterns
	if strings.Contains(strings.ToLower(absPath), "secret") ||
		strings.Contains(strings.ToLower(absPath), "private") ||
		strings.Contains(strings.ToLower(absPath), ".ssh") {
		return fmt.Errorf("path contains sensitive patterns: %s", path)
	}

	return nil
}

// Operation-specific validations

// validateCopyOperation validates copy operations
func validateCopyOperation(paths []string) error {
	if len(paths) < 2 {
		return fmt.Errorf("copy operation requires source and destination paths")
	}

	src, dest := paths[0], paths[1]

	// Source must be readable
	if currentSecurityContext.Level >= SecurityHigh {
		// In high security, verify source is accessible
		if !isPathAccessible(src) {
			return fmt.Errorf("source path not accessible: %s", src)
		}
	}

	// Destination must be writable
	if currentSecurityContext.Level >= SecurityHigh {
		if !isPathWritable(dest) {
			return fmt.Errorf("destination path not writable: %s", dest)
		}
	}

	return nil
}

// validateCreateOperation validates directory creation
func validateCreateOperation(paths []string) error {
	if len(paths) < 1 {
		return fmt.Errorf("create operation requires path")
	}

	path := paths[0]

	// Check if parent is writable
	if currentSecurityContext.Level >= SecurityHigh {
		parent := filepath.Dir(path)
		if !isPathWritable(parent) {
			return fmt.Errorf("parent directory not writable: %s", parent)
		}
	}

	return nil
}

// validateRemoveOperation validates removal operations
func validateRemoveOperation(paths []string) error {
	if len(paths) < 1 {
		return fmt.Errorf("remove operation requires path")
	}

	path := paths[0]

	// Strict mode prevents removal of important paths
	if currentSecurityContext.Level >= SecurityStrict {
		if strings.HasSuffix(path, "/") || path == "." || path == ".." {
			return fmt.Errorf("removal of directory roots not allowed: %s", path)
		}
	}

	return nil
}

// validateCommandOperation validates command execution
func validateCommandOperation(paths []string) error {
	// Command execution may be restricted in WASI
	if currentSecurityContext.Level >= SecurityHigh {
		return fmt.Errorf("command execution restricted in high security mode")
	}

	return nil
}

// Helper functions

// isPathAccessible checks if a path is accessible for reading
func isPathAccessible(path string) bool {
	if len(currentSecurityContext.AccessibleDirs) == 0 {
		return true // Allow if no restrictions
	}
	return isWithinAny(currentSecurityContext.AccessibleDirs, path)
}

// isPathWritable checks if a path is writable
func isPathWritable(path string) bool {
	// In a real implementation, this would check WASI permissions
	// For now, use the same logic as accessible
	return isPathAccessible(path)
}

// SetSecurityLevel updates the current security level
func SetSecurityLevel(level SecurityLevel) {
	currentSecurityContext.Level = level

	// Update restrictions based on level
	switch level {
	case SecurityStandard:
		currentSecurityContext.Restrictions = []string{"basic path traversal protection"}
	case SecurityHigh:
		currentSecurityContext.Restrictions = append(currentSecurityContext.Restrictions,
			"directory access restrictions", "preopen directory enforcement")
	case SecurityStrict:
		currentSecurityContext.Restrictions = append(currentSecurityContext.Restrictions,
			"explicit allow-listing required", "sensitive path detection")
	}
}
