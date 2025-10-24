// Package main provides tests for core file operations
package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCopyFile(t *testing.T) {
	// Create temporary directory
	tempDir := t.TempDir()

	// Create test source file
	srcPath := filepath.Join(tempDir, "source.txt")
	srcContent := "Hello, World!"
	if err := os.WriteFile(srcPath, []byte(srcContent), 0644); err != nil {
		t.Fatalf("Failed to create source file: %v", err)
	}

	// Test copying to different destination
	destPath := filepath.Join(tempDir, "dest.txt")
	if err := CopyFile(srcPath, destPath); err != nil {
		t.Fatalf("CopyFile failed: %v", err)
	}

	// Verify destination exists and has correct content
	destContent, err := os.ReadFile(destPath)
	if err != nil {
		t.Fatalf("Failed to read destination file: %v", err)
	}

	if string(destContent) != srcContent {
		t.Errorf("Content mismatch: got %q, want %q", string(destContent), srcContent)
	}
}

func TestCopyFileToSubdirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Create source file
	srcPath := filepath.Join(tempDir, "source.txt")
	if err := os.WriteFile(srcPath, []byte("test content"), 0644); err != nil {
		t.Fatalf("Failed to create source file: %v", err)
	}

	// Copy to subdirectory (should create parent dirs)
	destPath := filepath.Join(tempDir, "subdir", "dest.txt")
	if err := CopyFile(srcPath, destPath); err != nil {
		t.Fatalf("CopyFile to subdirectory failed: %v", err)
	}

	// Verify file exists
	if _, err := os.Stat(destPath); os.IsNotExist(err) {
		t.Error("Destination file was not created")
	}
}

func TestCreateDirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Test creating single directory
	dirPath := filepath.Join(tempDir, "testdir")
	if err := CreateDirectory(dirPath); err != nil {
		t.Fatalf("CreateDirectory failed: %v", err)
	}

	// Verify directory exists
	info, err := os.Stat(dirPath)
	if err != nil {
		t.Fatalf("Directory was not created: %v", err)
	}
	if !info.IsDir() {
		t.Error("Created path is not a directory")
	}
}

func TestCreateNestedDirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Test creating nested directories (mkdir -p behavior)
	nestedPath := filepath.Join(tempDir, "level1", "level2", "level3")
	if err := CreateDirectory(nestedPath); err != nil {
		t.Fatalf("CreateDirectory for nested path failed: %v", err)
	}

	// Verify all levels exist
	if _, err := os.Stat(nestedPath); os.IsNotExist(err) {
		t.Error("Nested directory was not created")
	}
}

func TestCopyDirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Create source directory with files
	srcDir := filepath.Join(tempDir, "source")
	if err := os.MkdirAll(srcDir, 0755); err != nil {
		t.Fatalf("Failed to create source directory: %v", err)
	}

	// Add some files
	files := map[string]string{
		"file1.txt":        "content1",
		"file2.txt":        "content2",
		"subdir/file3.txt": "content3",
	}

	for filePath, content := range files {
		fullPath := filepath.Join(srcDir, filePath)
		if err := os.MkdirAll(filepath.Dir(fullPath), 0755); err != nil {
			t.Fatalf("Failed to create subdirectory: %v", err)
		}
		if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to create test file: %v", err)
		}
	}

	// Copy directory
	destDir := filepath.Join(tempDir, "dest")
	if err := CopyDirectory(srcDir, destDir); err != nil {
		t.Fatalf("CopyDirectory failed: %v", err)
	}

	// Verify all files were copied
	for filePath, expectedContent := range files {
		destFilePath := filepath.Join(destDir, filePath)
		content, err := os.ReadFile(destFilePath)
		if err != nil {
			t.Errorf("Failed to read copied file %s: %v", filePath, err)
			continue
		}
		if string(content) != expectedContent {
			t.Errorf("Content mismatch in %s: got %q, want %q", filePath, string(content), expectedContent)
		}
	}
}

func TestPathExists(t *testing.T) {
	tempDir := t.TempDir()

	// Test non-existent path
	nonExistentPath := filepath.Join(tempDir, "nonexistent")
	if PathExists(nonExistentPath) != PathNotFound {
		t.Error("PathExists should return PathNotFound for non-existent path")
	}

	// Test file
	filePath := filepath.Join(tempDir, "testfile.txt")
	if err := os.WriteFile(filePath, []byte("test"), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}
	if PathExists(filePath) != PathFile {
		t.Error("PathExists should return PathFile for regular file")
	}

	// Test directory
	dirPath := filepath.Join(tempDir, "testdir")
	if err := os.MkdirAll(dirPath, 0755); err != nil {
		t.Fatalf("Failed to create test directory: %v", err)
	}
	if PathExists(dirPath) != PathDirectory {
		t.Error("PathExists should return PathDirectory for directory")
	}
}

func TestResolveAbsolutePath(t *testing.T) {
	// Test relative path resolution
	relativePath := "test/path"
	absPath, err := ResolveAbsolutePath(relativePath)
	if err != nil {
		t.Fatalf("ResolveAbsolutePath failed: %v", err)
	}

	if !filepath.IsAbs(absPath) {
		t.Errorf("ResolveAbsolutePath should return absolute path, got: %s", absPath)
	}

	if !strings.HasSuffix(absPath, relativePath) {
		t.Errorf("Resolved path should end with relative path: %s", absPath)
	}
}

func TestJoinPaths(t *testing.T) {
	tests := []struct {
		input    []string
		expected string
	}{
		{[]string{"a", "b", "c"}, filepath.Join("a", "b", "c")},
		{[]string{"", "b", "c"}, filepath.Join("", "b", "c")},
		{[]string{"a"}, "a"},
		{[]string{}, ""},
	}

	for _, test := range tests {
		result := JoinPaths(test.input)
		if result != test.expected {
			t.Errorf("JoinPaths(%v) = %q, want %q", test.input, result, test.expected)
		}
	}
}

func TestGetDirname(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"/path/to/file.txt", "/path/to"},
		{"file.txt", "."},
		{"/path/to/", "/path/to"},
		{"", "."},
	}

	for _, test := range tests {
		result := GetDirname(test.input)
		if result != test.expected {
			t.Errorf("GetDirname(%q) = %q, want %q", test.input, result, test.expected)
		}
	}
}

func TestGetBasename(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"/path/to/file.txt", "file.txt"},
		{"file.txt", "file.txt"},
		{"/path/to/", "to"},
		{"", "."},
	}

	for _, test := range tests {
		result := GetBasename(test.input)
		if result != test.expected {
			t.Errorf("GetBasename(%q) = %q, want %q", test.input, result, test.expected)
		}
	}
}

func TestListDirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Create test files
	testFiles := []string{"file1.txt", "file2.log", "file3.txt", "subdir"}
	for _, fileName := range testFiles {
		path := filepath.Join(tempDir, fileName)
		if fileName == "subdir" {
			if err := os.MkdirAll(path, 0755); err != nil {
				t.Fatalf("Failed to create test directory: %v", err)
			}
		} else {
			if err := os.WriteFile(path, []byte("test"), 0644); err != nil {
				t.Fatalf("Failed to create test file: %v", err)
			}
		}
	}

	// Test listing all files
	files, err := ListDirectory(tempDir, nil)
	if err != nil {
		t.Fatalf("ListDirectory failed: %v", err)
	}

	if len(files) != len(testFiles) {
		t.Errorf("Expected %d files, got %d", len(testFiles), len(files))
	}

	// Test pattern matching
	pattern := "*.txt"
	txtFiles, err := ListDirectory(tempDir, &pattern)
	if err != nil {
		t.Fatalf("ListDirectory with pattern failed: %v", err)
	}

	expectedTxtFiles := 2 // file1.txt, file3.txt
	if len(txtFiles) != expectedTxtFiles {
		t.Errorf("Expected %d .txt files, got %d", expectedTxtFiles, len(txtFiles))
	}
}

func TestRemovePath(t *testing.T) {
	tempDir := t.TempDir()

	// Test removing file
	filePath := filepath.Join(tempDir, "testfile.txt")
	if err := os.WriteFile(filePath, []byte("test"), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	if err := RemovePath(filePath); err != nil {
		t.Fatalf("RemovePath failed: %v", err)
	}

	if PathExists(filePath) != PathNotFound {
		t.Error("File should have been removed")
	}

	// Test removing non-existent file (should not error)
	if err := RemovePath(filePath); err != nil {
		t.Errorf("RemovePath should not error on non-existent file: %v", err)
	}
}
