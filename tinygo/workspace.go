// Package main provides workspace management operations for build systems
// Implements the workspace-management WIT interface for complex workspace preparation
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// WorkspaceConfig represents configuration for workspace preparation
type WorkspaceConfig struct {
	WorkDir        string         `json:"work_dir"`
	Sources        []FileSpec     `json:"sources"`
	Headers        []FileSpec     `json:"headers"`
	BindingsDir    *string        `json:"bindings_dir,omitempty"`
	Dependencies   []FileSpec     `json:"dependencies"`
	WorkspaceType  WorkspaceType  `json:"workspace_type"`
	SecurityConfig *SecurityConfig `json:"security_config,omitempty"`
}

// FileSpec represents a file specification with source and destination
type FileSpec struct {
	Source            string `json:"source"`
	Destination       *string `json:"destination,omitempty"`
	PreservePermissions bool  `json:"preserve_permissions"`
	PreserveStructure  bool   `json:"preserve_structure"`
}

// WorkspaceType represents different types of workspaces
type WorkspaceType int

const (
	WorkspaceRust WorkspaceType = iota
	WorkspaceGo
	WorkspaceCpp
	WorkspaceJavaScript
	WorkspaceGeneric
)

// PackageConfig represents package.json configuration for JavaScript builds
type PackageConfig struct {
	Name             string       `json:"name"`
	Version          string       `json:"version"`
	ModuleType       string       `json:"module_type"`
	Dependencies     []Dependency `json:"dependencies"`
	AdditionalFields []JsonField  `json:"additional_fields"`
}

// GoModuleConfig represents Go module configuration for TinyGo builds
type GoModuleConfig struct {
	ModuleName string     `json:"module_name"`
	GoVersion  string     `json:"go_version"`
	Sources    []FileSpec `json:"sources"`
	GoModFile  *string    `json:"go_mod_file,omitempty"`
	WitFile    *string    `json:"wit_file,omitempty"`
}

// CppWorkspaceConfig represents C/C++ workspace configuration
type CppWorkspaceConfig struct {
	Sources           []FileSpec `json:"sources"`
	Headers           []FileSpec `json:"headers"`
	BindingsDir       *string    `json:"bindings_dir,omitempty"`
	DependencyHeaders []FileSpec `json:"dependency_headers"`
}

// Dependency represents an NPM dependency
type Dependency struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// JsonField represents a generic JSON field
type JsonField struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// PrepareWorkspace prepares a complete workspace from configuration
// Implements the prepare-workspace WIT interface function
func PrepareWorkspace(config WorkspaceConfig) (WorkspaceInfo, error) {
	timer := NewOperationTimer()

	// Apply security configuration if provided
	if config.SecurityConfig != nil {
		SetSecurityLevel(config.SecurityConfig.Level)
	}

	// Create working directory
	if err := CreateDirectory(config.WorkDir); err != nil {
		return WorkspaceInfo{}, fmt.Errorf("failed to create workspace directory: %w", err)
	}

	var preparedFiles []string

	// Copy source files
	for _, source := range config.Sources {
		files, err := copyFileSpec(source, config.WorkDir)
		if err != nil {
			return WorkspaceInfo{}, fmt.Errorf("failed to copy source file: %w", err)
		}
		preparedFiles = append(preparedFiles, files...)
	}

	// Copy header files
	for _, header := range config.Headers {
		files, err := copyFileSpec(header, config.WorkDir)
		if err != nil {
			return WorkspaceInfo{}, fmt.Errorf("failed to copy header file: %w", err)
		}
		preparedFiles = append(preparedFiles, files...)
	}

	// Copy dependency files
	for _, dep := range config.Dependencies {
		files, err := copyFileSpec(dep, config.WorkDir)
		if err != nil {
			return WorkspaceInfo{}, fmt.Errorf("failed to copy dependency file: %w", err)
		}
		preparedFiles = append(preparedFiles, files...)
	}

	// Copy bindings directory if specified
	if config.BindingsDir != nil {
		if PathExists(*config.BindingsDir) != PathNotFound {
			if err := CopyDirectory(*config.BindingsDir, config.WorkDir); err != nil {
				return WorkspaceInfo{}, fmt.Errorf("failed to copy bindings directory: %w", err)
			}
			preparedFiles = append(preparedFiles, fmt.Sprintf("%s/* (bindings)", config.WorkDir))
		}
	}

	workspaceTypeStr := getWorkspaceTypeString(config.WorkspaceType)

	return WorkspaceInfo{
		PreparedFiles:     preparedFiles,
		WorkspacePath:     config.WorkDir,
		Message:           fmt.Sprintf("Successfully prepared %s workspace with %d files", workspaceTypeStr, len(preparedFiles)),
		PreparationTimeMs: timer.ElapsedMs(),
	}, nil
}

// CopySources copies source files to workspace with proper organization
// Implements the copy-sources WIT interface function
func CopySources(sources []FileSpec, destDir string) error {
	for _, source := range sources {
		_, err := copyFileSpec(source, destDir)
		if err != nil {
			return fmt.Errorf("failed to copy source %s: %w", source.Source, err)
		}
	}
	return nil
}

// CopyHeaders copies header files to workspace
// Implements the copy-headers WIT interface function
func CopyHeaders(headers []FileSpec, destDir string) error {
	for _, header := range headers {
		_, err := copyFileSpec(header, destDir)
		if err != nil {
			return fmt.Errorf("failed to copy header %s: %w", header.Source, err)
		}
	}
	return nil
}

// CopyBindings copies generated bindings to workspace
// Implements the copy-bindings WIT interface function
func CopyBindings(bindingsDir, destDir string) error {
	return CopyDirectory(bindingsDir, destDir)
}

// SetupPackageJson sets up package.json for JavaScript/Node.js builds
// Implements the setup-package-json WIT interface function
func SetupPackageJson(config PackageConfig, workDir string) error {
	packageData := map[string]interface{}{
		"name":    config.Name,
		"version": config.Version,
		"type":    config.ModuleType,
	}

	// Add dependencies
	if len(config.Dependencies) > 0 {
		deps := make(map[string]string)
		for _, dep := range config.Dependencies {
			deps[dep.Name] = dep.Version
		}
		packageData["dependencies"] = deps
	}

	// Add additional fields
	for _, field := range config.AdditionalFields {
		var value interface{}
		if err := json.Unmarshal([]byte(field.Value), &value); err != nil {
			// If not valid JSON, treat as string
			value = field.Value
		}
		packageData[field.Key] = value
	}

	// Write package.json
	packageJson, err := json.MarshalIndent(packageData, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal package.json: %w", err)
	}

	packagePath := filepath.Join(workDir, "package.json")
	if err := os.WriteFile(packagePath, packageJson, 0644); err != nil {
		return fmt.Errorf("failed to write package.json: %w", err)
	}

	return nil
}

// SetupGoModule organizes Go module structure for TinyGo builds
// Implements the setup-go-module WIT interface function
func SetupGoModule(config GoModuleConfig, workDir string) error {
	// Copy source files
	for _, source := range config.Sources {
		if _, err := copyFileSpec(source, workDir); err != nil {
			return fmt.Errorf("failed to copy Go source: %w", err)
		}
	}

	// Copy go.mod file if provided
	if config.GoModFile != nil {
		goModDest := filepath.Join(workDir, "go.mod")
		if err := CopyFile(*config.GoModFile, goModDest); err != nil {
			return fmt.Errorf("failed to copy go.mod: %w", err)
		}
	} else {
		// Create basic go.mod
		goModContent := fmt.Sprintf("module %s\n\ngo %s\n", config.ModuleName, config.GoVersion)
		goModPath := filepath.Join(workDir, "go.mod")
		if err := os.WriteFile(goModPath, []byte(goModContent), 0644); err != nil {
			return fmt.Errorf("failed to create go.mod: %w", err)
		}
	}

	// Copy WIT file if provided
	if config.WitFile != nil {
		witDest := filepath.Join(workDir, "component.wit")
		if err := CopyFile(*config.WitFile, witDest); err != nil {
			return fmt.Errorf("failed to copy WIT file: %w", err)
		}
	}

	return nil
}

// SetupCppWorkspace organizes C/C++ source structure for compilation
// Implements the setup-cpp-workspace WIT interface function
func SetupCppWorkspace(config CppWorkspaceConfig, workDir string) error {
	// Copy source files
	for _, source := range config.Sources {
		if _, err := copyFileSpec(source, workDir); err != nil {
			return fmt.Errorf("failed to copy C++ source: %w", err)
		}
	}

	// Copy header files with structure preservation
	for _, header := range config.Headers {
		if _, err := copyFileSpec(header, workDir); err != nil {
			return fmt.Errorf("failed to copy C++ header: %w", err)
		}
	}

	// Copy dependency headers
	for _, depHeader := range config.DependencyHeaders {
		if _, err := copyFileSpec(depHeader, workDir); err != nil {
			return fmt.Errorf("failed to copy dependency header: %w", err)
		}
	}

	// Copy bindings directory if specified
	if config.BindingsDir != nil {
		bindingsPath := filepath.Join(workDir, "bindings")
		if err := CopyDirectory(*config.BindingsDir, bindingsPath); err != nil {
			return fmt.Errorf("failed to copy bindings: %w", err)
		}
	}

	return nil
}

// Helper functions

// copyFileSpec copies a file according to FileSpec configuration
func copyFileSpec(spec FileSpec, destDir string) ([]string, error) {
	// Determine destination name
	var destName string
	if spec.Destination != nil {
		destName = *spec.Destination
	} else {
		if spec.PreserveStructure {
			// Keep relative path structure
			destName = spec.Source
		} else {
			// Just use basename
			destName = filepath.Base(spec.Source)
		}
	}

	// Handle directory structure preservation for headers
	if spec.PreserveStructure && strings.Contains(destName, "/") {
		// For files like "test/cross_package_headers/foundation/types.h"
		// we want to preserve "foundation/types.h" structure
		pathParts := strings.Split(destName, "/")
		if len(pathParts) >= 2 {
			// Take the last 2 parts to preserve subdirectory structure
			destName = filepath.Join(pathParts[len(pathParts)-2:]...)
		}
	}

	destPath := filepath.Join(destDir, destName)

	// Copy the file
	if err := CopyFile(spec.Source, destPath); err != nil {
		return nil, err
	}

	return []string{destPath}, nil
}

// getWorkspaceTypeString converts WorkspaceType to string
func getWorkspaceTypeString(wsType WorkspaceType) string {
	switch wsType {
	case WorkspaceRust:
		return "Rust"
	case WorkspaceGo:
		return "Go"
	case WorkspaceCpp:
		return "C++"
	case WorkspaceJavaScript:
		return "JavaScript"
	case WorkspaceGeneric:
		return "Generic"
	default:
		return "Unknown"
	}
}