// Package main provides JSON batch processing for backward compatibility
// Bridges the gap between existing JSON configurations and the new WIT interface
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// JsonConfig represents the JSON configuration for batch file operations
// This maintains compatibility with the original Go implementation
type JsonConfig struct {
	WorkspaceDir string      `json:"workspace_dir"`
	Operations   []Operation `json:"operations"`
}

// Operation represents a single file operation from JSON config
type Operation struct {
	Type       string   `json:"type"`
	SrcPath    string   `json:"src_path,omitempty"`
	DestPath   string   `json:"dest_path,omitempty"`
	Path       string   `json:"path,omitempty"`
	Command    string   `json:"command,omitempty"`
	Args       []string `json:"args,omitempty"`
	WorkDir    string   `json:"work_dir,omitempty"`
	OutputFile string   `json:"output_file,omitempty"`
}

// WorkspaceInfo represents the result of workspace operations
type WorkspaceInfo struct {
	PreparedFiles     []string `json:"prepared_files"`
	WorkspacePath     string   `json:"workspace_path"`
	Message           string   `json:"message"`
	PreparationTimeMs uint64   `json:"preparation_time_ms"`
}

// ProcessJsonConfig processes a JSON configuration for batch file operations
// Implements the process-json-config WIT interface function
func ProcessJsonConfig(configJson string) (WorkspaceInfo, error) {
	timer := NewOperationTimer()

	// Parse JSON configuration
	var config JsonConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return WorkspaceInfo{}, fmt.Errorf("failed to parse JSON config: %w", err)
	}

	// Validate configuration
	if err := validateJsonConfig(config); err != nil {
		return WorkspaceInfo{}, fmt.Errorf("invalid JSON config: %w", err)
	}

	// Create workspace directory
	if err := CreateDirectory(config.WorkspaceDir); err != nil {
		return WorkspaceInfo{}, fmt.Errorf("failed to create workspace directory: %w", err)
	}

	var preparedFiles []string

	// Execute operations in sequence
	for i, op := range config.Operations {
		files, err := executeJsonOperation(op, config.WorkspaceDir)
		if err != nil {
			return WorkspaceInfo{}, fmt.Errorf("operation %d failed: %w", i, err)
		}
		preparedFiles = append(preparedFiles, files...)
	}

	return WorkspaceInfo{
		PreparedFiles:     preparedFiles,
		WorkspacePath:     config.WorkspaceDir,
		Message:           fmt.Sprintf("Successfully processed %d operations", len(config.Operations)),
		PreparationTimeMs: timer.ElapsedMs(),
	}, nil
}

// ValidateJsonConfig validates a JSON configuration before processing
// Implements the validate-json-config WIT interface function
func ValidateJsonConfig(configJson string) error {
	var config JsonConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return fmt.Errorf("failed to parse JSON config: %w", err)
	}

	return validateJsonConfig(config)
}

// GetJsonSchema returns the JSON schema for configuration validation
// Implements the get-json-schema WIT interface function
func GetJsonSchema() string {
	schema := `{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["workspace_dir", "operations"],
  "properties": {
    "workspace_dir": {
      "type": "string",
      "description": "Absolute path to workspace directory"
    },
    "operations": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["type"],
        "properties": {
          "type": {
            "type": "string",
            "enum": ["copy_file", "mkdir", "copy_directory_contents", "run_command"]
          },
          "src_path": {"type": "string"},
          "dest_path": {"type": "string"},
          "path": {"type": "string"},
          "command": {"type": "string"},
          "args": {"type": "array", "items": {"type": "string"}},
          "work_dir": {"type": "string"},
          "output_file": {"type": "string"}
        }
      }
    }
  }
}`
	return schema
}

// Helper functions

// validateJsonConfig performs validation on JSON configuration
func validateJsonConfig(config JsonConfig) error {
	if config.WorkspaceDir == "" {
		return fmt.Errorf("workspace_dir cannot be empty")
	}

	if !filepath.IsAbs(config.WorkspaceDir) {
		return fmt.Errorf("workspace_dir must be an absolute path: %s", config.WorkspaceDir)
	}

	for i, op := range config.Operations {
		if err := validateOperation(op, i); err != nil {
			return err
		}
	}

	return nil
}

// validateOperation validates a single operation
func validateOperation(op Operation, index int) error {
	switch op.Type {
	case "copy_file":
		if op.SrcPath == "" || op.DestPath == "" {
			return fmt.Errorf("operation %d: copy_file requires src_path and dest_path", index)
		}
		if !filepath.IsAbs(op.SrcPath) {
			return fmt.Errorf("operation %d: src_path must be absolute: %s", index, op.SrcPath)
		}
		if filepath.IsAbs(op.DestPath) {
			return fmt.Errorf("operation %d: dest_path must be relative: %s", index, op.DestPath)
		}
	case "mkdir":
		if op.Path == "" {
			return fmt.Errorf("operation %d: mkdir requires path", index)
		}
		if filepath.IsAbs(op.Path) {
			return fmt.Errorf("operation %d: mkdir path must be relative: %s", index, op.Path)
		}
	case "copy_directory_contents":
		if op.SrcPath == "" || op.DestPath == "" {
			return fmt.Errorf("operation %d: copy_directory_contents requires src_path and dest_path", index)
		}
		if !filepath.IsAbs(op.SrcPath) {
			return fmt.Errorf("operation %d: src_path must be absolute: %s", index, op.SrcPath)
		}
		if filepath.IsAbs(op.DestPath) {
			return fmt.Errorf("operation %d: dest_path must be relative: %s", index, op.DestPath)
		}
	case "run_command":
		if op.Command == "" {
			return fmt.Errorf("operation %d: run_command requires command", index)
		}
	default:
		return fmt.Errorf("operation %d: unknown operation type: %s", index, op.Type)
	}

	return nil
}

// executeJsonOperation executes a single JSON operation
func executeJsonOperation(op Operation, workspaceDir string) ([]string, error) {
	switch op.Type {
	case "copy_file":
		return executeJsonCopyFile(op, workspaceDir)
	case "mkdir":
		return executeJsonMkdir(op, workspaceDir)
	case "copy_directory_contents":
		return executeJsonCopyDirectoryContents(op, workspaceDir)
	case "run_command":
		return executeJsonRunCommand(op, workspaceDir)
	default:
		return nil, fmt.Errorf("unsupported operation type: %s", op.Type)
	}
}

// executeJsonCopyFile executes copy_file operation
func executeJsonCopyFile(op Operation, workspaceDir string) ([]string, error) {
	dest := filepath.Join(workspaceDir, op.DestPath)

	if err := CopyFile(op.SrcPath, dest); err != nil {
		return nil, err
	}

	return []string{dest}, nil
}

// executeJsonMkdir executes mkdir operation
func executeJsonMkdir(op Operation, workspaceDir string) ([]string, error) {
	path := filepath.Join(workspaceDir, op.Path)

	if err := CreateDirectory(path); err != nil {
		return nil, err
	}

	return []string{path}, nil
}

// executeJsonCopyDirectoryContents executes copy_directory_contents operation
func executeJsonCopyDirectoryContents(op Operation, workspaceDir string) ([]string, error) {
	dest := filepath.Join(workspaceDir, op.DestPath)

	if err := CopyDirectory(op.SrcPath, dest); err != nil {
		return nil, err
	}

	// List all files that were copied (for reporting)
	files, err := ListDirectory(dest, nil)
	if err != nil {
		// Don't fail the operation if listing fails
		return []string{dest}, nil
	}

	var fullPaths []string
	for _, file := range files {
		fullPaths = append(fullPaths, filepath.Join(dest, file))
	}

	return fullPaths, nil
}

// executeJsonRunCommand executes run_command operation
// Note: This may be limited in WASI environment
func executeJsonRunCommand(op Operation, workspaceDir string) ([]string, error) {
	// Determine working directory
	workDir := workspaceDir
	if op.WorkDir != "" {
		if filepath.IsAbs(op.WorkDir) {
			workDir = op.WorkDir
		} else {
			workDir = filepath.Join(workspaceDir, op.WorkDir)
		}
	}

	// Create command
	cmd := exec.Command(op.Command, op.Args...)
	cmd.Dir = workDir

	// Handle output
	if op.OutputFile != "" {
		outputPath := filepath.Join(workspaceDir, op.OutputFile)

		// Ensure output directory exists
		if err := CreateDirectory(filepath.Dir(outputPath)); err != nil {
			return nil, fmt.Errorf("failed to create output directory: %w", err)
		}

		// Execute command and capture output
		output, err := cmd.Output()
		if err != nil {
			return nil, fmt.Errorf("command failed: %w", err)
		}

		// Write output to file
		if err := os.WriteFile(outputPath, output, 0644); err != nil {
			return nil, fmt.Errorf("failed to write output file: %w", err)
		}

		return []string{outputPath}, nil
	}

	// Execute command without capturing output
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("command failed: %w", err)
	}

	return []string{}, nil
}
