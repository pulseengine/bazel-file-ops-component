// Package main provides core file operations ported from the original Go implementation
// Optimized for TinyGo with WASI support and enhanced security validation.
package main

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// PathInfo represents the type of path (file, directory, etc.)
type PathInfo int

const (
	PathNotFound PathInfo = iota
	PathFile
	PathDirectory
	PathSymlink
	PathOther
)

// CopyFile copies a single file from source to destination
// Implements the copy-file WIT interface function
func CopyFile(src, dest string) error {
	// Security validation
	if err := ValidatePath(dest, []string{}); err != nil {
		return fmt.Errorf("security validation failed: %w", err)
	}

	// Ensure destination directory exists
	destDir := filepath.Dir(dest)
	if err := os.MkdirAll(destDir, 0755); err != nil {
		return fmt.Errorf("failed to create destination directory %s: %w", destDir, err)
	}

	// Open source file
	srcFile, err := os.Open(src)
	if err != nil {
		return fmt.Errorf("failed to open source file %s: %w", src, err)
	}
	defer srcFile.Close()

	// Create destination file
	destFile, err := os.Create(dest)
	if err != nil {
		return fmt.Errorf("failed to create destination file %s: %w", dest, err)
	}
	defer destFile.Close()

	// Copy file contents
	_, err = io.Copy(destFile, srcFile)
	if err != nil {
		return fmt.Errorf("failed to copy file contents: %w", err)
	}

	return nil
}

// CopyDirectory copies a directory recursively from source to destination
// Implements the copy-directory WIT interface function
func CopyDirectory(src, dest string) error {
	// Security validation
	if err := ValidatePath(dest, []string{}); err != nil {
		return fmt.Errorf("security validation failed: %w", err)
	}

	// Check source exists and is directory
	srcInfo, err := os.Stat(src)
	if err != nil {
		return fmt.Errorf("source directory does not exist: %s", src)
	}
	if !srcInfo.IsDir() {
		return fmt.Errorf("source is not a directory: %s", src)
	}

	// Create destination directory
	if err := os.MkdirAll(dest, srcInfo.Mode()); err != nil {
		return fmt.Errorf("failed to create destination directory %s: %w", dest, err)
	}

	// Copy directory contents recursively
	return copyDirectoryContents(src, dest)
}

// CreateDirectory creates a directory and all parent directories if needed
// Implements the create-directory WIT interface function
func CreateDirectory(path string) error {
	// Security validation
	if err := ValidatePath(path, []string{}); err != nil {
		return fmt.Errorf("security validation failed: %w", err)
	}

	if err := os.MkdirAll(path, 0755); err != nil {
		return fmt.Errorf("failed to create directory %s: %w", path, err)
	}

	return nil
}

// RemovePath removes a file or directory recursively
// Implements the remove-path WIT interface function
func RemovePath(path string) error {
	// Security validation
	if err := ValidatePath(path, []string{}); err != nil {
		return fmt.Errorf("security validation failed: %w", err)
	}

	if err := os.RemoveAll(path); err != nil {
		// Don't error on missing files - this is a "safe" operation
		if !os.IsNotExist(err) {
			return fmt.Errorf("failed to remove path %s: %w", path, err)
		}
	}

	return nil
}

// PathExists checks if a path exists and returns its type
// Implements the path-exists WIT interface function
func PathExists(path string) PathInfo {
	info, err := os.Lstat(path)
	if err != nil {
		return PathNotFound
	}

	switch {
	case info.Mode().IsRegular():
		return PathFile
	case info.Mode().IsDir():
		return PathDirectory
	case info.Mode()&os.ModeSymlink != 0:
		return PathSymlink
	default:
		return PathOther
	}
}

// ResolveAbsolutePath resolves a relative path to an absolute path
// Implements the resolve-absolute-path WIT interface function
func ResolveAbsolutePath(path string) (string, error) {
	absPath, err := filepath.Abs(path)
	if err != nil {
		return "", fmt.Errorf("failed to resolve absolute path for %s: %w", path, err)
	}
	return absPath, nil
}

// JoinPaths joins multiple path segments using the OS-appropriate separator
// Implements the join-paths WIT interface function
func JoinPaths(paths []string) string {
	return filepath.Join(paths...)
}

// GetDirname returns the directory name from a file path
// Implements the get-dirname WIT interface function
func GetDirname(path string) string {
	return filepath.Dir(path)
}

// GetBasename returns the filename from a file path
// Implements the get-basename WIT interface function
func GetBasename(path string) string {
	return filepath.Base(path)
}

// ListDirectory lists files in a directory with optional pattern matching
// Implements the list-directory WIT interface function
func ListDirectory(dir string, pattern *string) ([]string, error) {
	// Security validation
	if err := ValidatePath(dir, []string{}); err != nil {
		return nil, fmt.Errorf("security validation failed: %w", err)
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("failed to read directory %s: %w", dir, err)
	}

	var result []string
	for _, entry := range entries {
		name := entry.Name()

		// Apply pattern matching if provided
		if pattern != nil {
			matched, err := filepath.Match(*pattern, name)
			if err != nil {
				return nil, fmt.Errorf("invalid pattern %s: %w", *pattern, err)
			}
			if !matched {
				continue
			}
		}

		result = append(result, name)
	}

	return result, nil
}

// Helper functions

// copyDirectoryContents recursively copies directory contents
func copyDirectoryContents(src, dest string) error {
	entries, err := os.ReadDir(src)
	if err != nil {
		return fmt.Errorf("failed to read source directory %s: %w", src, err)
	}

	for _, entry := range entries {
		srcPath := filepath.Join(src, entry.Name())
		destPath := filepath.Join(dest, entry.Name())

		if entry.IsDir() {
			// Get directory info for permissions
			info, err := entry.Info()
			if err != nil {
				return fmt.Errorf("failed to get directory info: %w", err)
			}

			// Create subdirectory
			if err := os.MkdirAll(destPath, info.Mode()); err != nil {
				return fmt.Errorf("failed to create subdirectory %s: %w", destPath, err)
			}

			// Recursively copy subdirectory
			if err := copyDirectoryContents(srcPath, destPath); err != nil {
				return err
			}
		} else {
			// Copy file
			if err := CopyFile(srcPath, destPath); err != nil {
				return fmt.Errorf("failed to copy file %s: %w", entry.Name(), err)
			}
		}
	}

	return nil
}

// Performance monitoring helpers

// OperationTimer tracks operation performance
type OperationTimer struct {
	start time.Time
}

// NewOperationTimer creates a new operation timer
func NewOperationTimer() *OperationTimer {
	return &OperationTimer{start: time.Now()}
}

// ElapsedMs returns elapsed time in milliseconds
func (t *OperationTimer) ElapsedMs() uint64 {
	return uint64(time.Since(t.start).Nanoseconds() / 1e6)
}

// containsPathTraversal checks for path traversal attempts
// This is a security helper function from the original implementation
func containsPathTraversal(path string) bool {
	cleaned := filepath.Clean(path)
	// Only reject paths that contain ".." after cleaning
	// Absolute paths are allowed - the issue is only with relative paths trying to escape
	return strings.Contains(cleaned, "..")
}
