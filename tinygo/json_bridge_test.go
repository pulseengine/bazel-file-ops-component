// Package main provides tests for JSON batch processing functionality
package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestProcessJsonConfig(t *testing.T) {
	tempDir := t.TempDir()

	// Create test source files
	srcDir := filepath.Join(tempDir, "src")
	if err := os.MkdirAll(srcDir, 0755); err != nil {
		t.Fatalf("Failed to create source directory: %v", err)
	}

	srcFile := filepath.Join(srcDir, "main.cpp")
	if err := os.WriteFile(srcFile, []byte("int main() { return 0; }"), 0644); err != nil {
		t.Fatalf("Failed to create source file: %v", err)
	}

	// Create workspace directory
	workspaceDir := filepath.Join(tempDir, "workspace")

	// Create JSON configuration
	config := JsonConfig{
		WorkspaceDir: workspaceDir,
		Operations: []Operation{
			{
				Type: "mkdir",
				Path: "include",
			},
			{
				Type:     "copy_file",
				SrcPath:  srcFile,
				DestPath: "main.cpp",
			},
		},
	}

	configJson, err := json.Marshal(config)
	if err != nil {
		t.Fatalf("Failed to marshal config: %v", err)
	}

	// Process configuration
	result, err := ProcessJsonConfig(string(configJson))
	if err != nil {
		t.Fatalf("ProcessJsonConfig failed: %v", err)
	}

	// Verify results
	if result.WorkspacePath != workspaceDir {
		t.Errorf("Wrong workspace path: got %s, want %s", result.WorkspacePath, workspaceDir)
	}

	if len(result.PreparedFiles) != 2 { // mkdir + copy_file
		t.Errorf("Expected 2 prepared files, got %d", len(result.PreparedFiles))
	}

	// Verify workspace directory was created
	if PathExists(workspaceDir) != PathDirectory {
		t.Error("Workspace directory was not created")
	}

	// Verify include directory was created
	includeDir := filepath.Join(workspaceDir, "include")
	if PathExists(includeDir) != PathDirectory {
		t.Error("Include directory was not created")
	}

	// Verify file was copied
	copiedFile := filepath.Join(workspaceDir, "main.cpp")
	if PathExists(copiedFile) != PathFile {
		t.Error("Source file was not copied")
	}

	// Verify file content
	content, err := os.ReadFile(copiedFile)
	if err != nil {
		t.Fatalf("Failed to read copied file: %v", err)
	}
	expectedContent := "int main() { return 0; }"
	if string(content) != expectedContent {
		t.Errorf("Content mismatch: got %q, want %q", string(content), expectedContent)
	}
}

func TestValidateJsonConfig(t *testing.T) {
	tempDir := t.TempDir()

	tests := []struct {
		name    string
		config  JsonConfig
		wantErr bool
	}{
		{
			name: "valid config",
			config: JsonConfig{
				WorkspaceDir: filepath.Join(tempDir, "workspace"),
				Operations: []Operation{
					{Type: "mkdir", Path: "include"},
					{Type: "copy_file", SrcPath: "/absolute/src", DestPath: "relative/dest"},
				},
			},
			wantErr: false,
		},
		{
			name: "empty workspace dir",
			config: JsonConfig{
				WorkspaceDir: "",
				Operations:   []Operation{{Type: "mkdir", Path: "test"}},
			},
			wantErr: true,
		},
		{
			name: "relative workspace dir",
			config: JsonConfig{
				WorkspaceDir: "relative/path",
				Operations:   []Operation{{Type: "mkdir", Path: "test"}},
			},
			wantErr: true,
		},
		{
			name: "copy_file missing src_path",
			config: JsonConfig{
				WorkspaceDir: filepath.Join(tempDir, "workspace"),
				Operations: []Operation{
					{Type: "copy_file", DestPath: "dest"},
				},
			},
			wantErr: true,
		},
		{
			name: "copy_file absolute dest_path",
			config: JsonConfig{
				WorkspaceDir: filepath.Join(tempDir, "workspace"),
				Operations: []Operation{
					{Type: "copy_file", SrcPath: "/absolute/src", DestPath: "/absolute/dest"},
				},
			},
			wantErr: true,
		},
		{
			name: "mkdir absolute path",
			config: JsonConfig{
				WorkspaceDir: filepath.Join(tempDir, "workspace"),
				Operations: []Operation{
					{Type: "mkdir", Path: "/absolute/path"},
				},
			},
			wantErr: true,
		},
		{
			name: "unknown operation type",
			config: JsonConfig{
				WorkspaceDir: filepath.Join(tempDir, "workspace"),
				Operations: []Operation{
					{Type: "unknown_operation"},
				},
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			configJson, err := json.Marshal(tt.config)
			if err != nil {
				t.Fatalf("Failed to marshal config: %v", err)
			}

			err = ValidateJsonConfig(string(configJson))
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateJsonConfig() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestGetJsonSchema(t *testing.T) {
	schema := GetJsonSchema()

	// Basic validation - should be valid JSON
	var schemaObj interface{}
	if err := json.Unmarshal([]byte(schema), &schemaObj); err != nil {
		t.Errorf("GetJsonSchema returned invalid JSON: %v", err)
	}

	// Should contain expected schema properties
	expectedStrings := []string{
		"workspace_dir",
		"operations",
		"copy_file",
		"mkdir",
		"copy_directory_contents",
		"run_command",
	}

	for _, expected := range expectedStrings {
		if !containsString(schema, expected) {
			t.Errorf("Schema should contain %s", expected)
		}
	}
}

func TestJsonConfigCopyDirectoryContents(t *testing.T) {
	tempDir := t.TempDir()

	// Create source directory with files
	srcDir := filepath.Join(tempDir, "headers")
	if err := os.MkdirAll(srcDir, 0755); err != nil {
		t.Fatalf("Failed to create source directory: %v", err)
	}

	// Add test files
	testFiles := map[string]string{
		"header1.h": "#pragma once\n// Header 1",
		"header2.h": "#pragma once\n// Header 2",
	}

	for fileName, content := range testFiles {
		filePath := filepath.Join(srcDir, fileName)
		if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to create test file %s: %v", fileName, err)
		}
	}

	// Create JSON config for copy_directory_contents
	workspaceDir := filepath.Join(tempDir, "workspace")
	config := JsonConfig{
		WorkspaceDir: workspaceDir,
		Operations: []Operation{
			{
				Type:     "copy_directory_contents",
				SrcPath:  srcDir,
				DestPath: "include",
			},
		},
	}

	configJson, err := json.Marshal(config)
	if err != nil {
		t.Fatalf("Failed to marshal config: %v", err)
	}

	// Process configuration
	result, err := ProcessJsonConfig(string(configJson))
	if err != nil {
		t.Fatalf("ProcessJsonConfig failed: %v", err)
	}

	// Verify files were copied
	for fileName, expectedContent := range testFiles {
		copiedFile := filepath.Join(workspaceDir, "include", fileName)
		if PathExists(copiedFile) != PathFile {
			t.Errorf("File %s was not copied", fileName)
			continue
		}

		content, err := os.ReadFile(copiedFile)
		if err != nil {
			t.Errorf("Failed to read copied file %s: %v", fileName, err)
			continue
		}

		if string(content) != expectedContent {
			t.Errorf("Content mismatch in %s: got %q, want %q", fileName, string(content), expectedContent)
		}
	}

	// Verify result includes copied files
	if len(result.PreparedFiles) == 0 {
		t.Error("No prepared files reported")
	}
}

func TestJsonConfigRunCommand(t *testing.T) {
	tempDir := t.TempDir()

	workspaceDir := filepath.Join(tempDir, "workspace")
	outputFile := "output.txt"

	// Create JSON config for run_command (using echo)
	config := JsonConfig{
		WorkspaceDir: workspaceDir,
		Operations: []Operation{
			{
				Type:       "run_command",
				Command:    "echo",
				Args:       []string{"Hello, World!"},
				OutputFile: outputFile,
			},
		},
	}

	configJson, err := json.Marshal(config)
	if err != nil {
		t.Fatalf("Failed to marshal config: %v", err)
	}

	// Process configuration
	result, err := ProcessJsonConfig(string(configJson))
	if err != nil {
		t.Fatalf("ProcessJsonConfig failed: %v", err)
	}

	// Verify output file was created
	outputPath := filepath.Join(workspaceDir, outputFile)
	if PathExists(outputPath) != PathFile {
		t.Error("Output file was not created")
	}

	// Verify output content
	content, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("Failed to read output file: %v", err)
	}

	expectedContent := "Hello, World!\n" // echo adds newline
	if string(content) != expectedContent {
		t.Errorf("Output content mismatch: got %q, want %q", string(content), expectedContent)
	}

	// Verify result reports output file
	if len(result.PreparedFiles) == 0 {
		t.Error("No prepared files reported")
	}
}

// Helper function
func containsString(haystack, needle string) bool {
	return len(haystack) >= len(needle) &&
		(haystack == needle || len(needle) == 0 ||
			findInString(haystack, needle) >= 0)
}

func findInString(s, substr string) int {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return i
		}
	}
	return -1
}
