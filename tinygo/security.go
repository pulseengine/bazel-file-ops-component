// Package main provides security operations for WASM sandboxing and path validation
// Implements enhanced security features for the file operations component
package main

import (
	"fmt"
	"path/filepath"
	"strings"
)

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
	// Basic path traversal check already done
	// Standard level allows most operations
	return nil
}

// validatePathHigh performs high security validation
func validatePathHigh(path string, allowedDirs []string) error {
	// Check if path is within allowed directories
	if len(allowedDirs) > 0 {
		allowed := false
		for _, allowedDir := range allowedDirs {
			if strings.HasPrefix(filepath.Clean(path), filepath.Clean(allowedDir)) {
				allowed = true
				break
			}
		}
		if !allowed {
			return fmt.Errorf("path %s not within allowed directories", path)
		}
	}

	// Check against current security context
	if len(currentSecurityContext.AccessibleDirs) > 0 {
		accessible := false
		for _, accessibleDir := range currentSecurityContext.AccessibleDirs {
			if strings.HasPrefix(filepath.Clean(path), filepath.Clean(accessibleDir)) {
				accessible = true
				break
			}
		}
		if !accessible {
			return fmt.Errorf("path %s not accessible in current security context", path)
		}
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
	for _, accessibleDir := range currentSecurityContext.AccessibleDirs {
		if strings.HasPrefix(filepath.Clean(path), filepath.Clean(accessibleDir)) {
			return true
		}
	}
	return len(currentSecurityContext.AccessibleDirs) == 0 // Allow if no restrictions
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
