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

func TestReadFile(t *testing.T) {
	tempDir := t.TempDir()

	// Create test file with content
	testPath := filepath.Join(tempDir, "test.txt")
	testContent := "Hello, World!\nLine 2\nLine 3"
	if err := os.WriteFile(testPath, []byte(testContent), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	// Test reading file
	content, err := ReadFile(testPath)
	if err != nil {
		t.Fatalf("ReadFile failed: %v", err)
	}

	if content != testContent {
		t.Errorf("Content mismatch: got %q, want %q", content, testContent)
	}

	// Test reading non-existent file
	_, err = ReadFile(filepath.Join(tempDir, "nonexistent.txt"))
	if err == nil {
		t.Error("ReadFile should fail for non-existent file")
	}
}

func TestWriteFile(t *testing.T) {
	tempDir := t.TempDir()

	// Test writing to new file
	testPath := filepath.Join(tempDir, "output.txt")
	testContent := "Test content\nSecond line"
	if err := WriteFile(testPath, testContent); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	// Verify file was created with correct content
	content, err := os.ReadFile(testPath)
	if err != nil {
		t.Fatalf("Failed to read written file: %v", err)
	}

	if string(content) != testContent {
		t.Errorf("Content mismatch: got %q, want %q", string(content), testContent)
	}

	// Test overwriting existing file
	newContent := "Overwritten content"
	if err := WriteFile(testPath, newContent); err != nil {
		t.Fatalf("WriteFile (overwrite) failed: %v", err)
	}

	content, err = os.ReadFile(testPath)
	if err != nil {
		t.Fatalf("Failed to read overwritten file: %v", err)
	}

	if string(content) != newContent {
		t.Errorf("Overwrite content mismatch: got %q, want %q", string(content), newContent)
	}
}

func TestWriteFileCreatesParentDir(t *testing.T) {
	tempDir := t.TempDir()

	// Test writing to file in non-existent subdirectory
	testPath := filepath.Join(tempDir, "subdir", "output.txt")
	testContent := "Test content"
	if err := WriteFile(testPath, testContent); err != nil {
		t.Fatalf("WriteFile should create parent directory: %v", err)
	}

	// Verify file exists
	if _, err := os.Stat(testPath); os.IsNotExist(err) {
		t.Error("File was not created")
	}
}

func TestAppendToFile(t *testing.T) {
	tempDir := t.TempDir()

	testPath := filepath.Join(tempDir, "append.txt")

	// Test appending to non-existent file (should create it)
	firstContent := "First line\n"
	if err := AppendToFile(testPath, firstContent); err != nil {
		t.Fatalf("AppendToFile (create) failed: %v", err)
	}

	// Verify file was created
	content, err := os.ReadFile(testPath)
	if err != nil {
		t.Fatalf("Failed to read file: %v", err)
	}
	if string(content) != firstContent {
		t.Errorf("Content mismatch: got %q, want %q", string(content), firstContent)
	}

	// Test appending to existing file
	secondContent := "Second line\n"
	if err := AppendToFile(testPath, secondContent); err != nil {
		t.Fatalf("AppendToFile (append) failed: %v", err)
	}

	// Verify content was appended
	content, err = os.ReadFile(testPath)
	if err != nil {
		t.Fatalf("Failed to read appended file: %v", err)
	}

	expectedContent := firstContent + secondContent
	if string(content) != expectedContent {
		t.Errorf("Appended content mismatch: got %q, want %q", string(content), expectedContent)
	}
}

func TestConcatenateFiles(t *testing.T) {
	tempDir := t.TempDir()

	// Create source files
	sources := []struct {
		path    string
		content string
	}{
		{filepath.Join(tempDir, "file1.txt"), "Content from file 1\n"},
		{filepath.Join(tempDir, "file2.txt"), "Content from file 2\n"},
		{filepath.Join(tempDir, "file3.txt"), "Content from file 3\n"},
	}

	for _, src := range sources {
		if err := os.WriteFile(src.path, []byte(src.content), 0644); err != nil {
			t.Fatalf("Failed to create source file %s: %v", src.path, err)
		}
	}

	// Concatenate files
	destPath := filepath.Join(tempDir, "concatenated.txt")
	sourcePaths := []string{sources[0].path, sources[1].path, sources[2].path}

	if err := ConcatenateFiles(sourcePaths, destPath); err != nil {
		t.Fatalf("ConcatenateFiles failed: %v", err)
	}

	// Verify concatenated content
	content, err := os.ReadFile(destPath)
	if err != nil {
		t.Fatalf("Failed to read concatenated file: %v", err)
	}

	expectedContent := sources[0].content + sources[1].content + sources[2].content
	if string(content) != expectedContent {
		t.Errorf("Concatenated content mismatch:\ngot: %q\nwant: %q", string(content), expectedContent)
	}
}

func TestConcatenateFilesEmptySources(t *testing.T) {
	tempDir := t.TempDir()
	destPath := filepath.Join(tempDir, "output.txt")

	// Test with empty sources list
	err := ConcatenateFiles([]string{}, destPath)
	if err == nil {
		t.Error("ConcatenateFiles should fail with empty sources")
	}
}

func TestMovePath(t *testing.T) {
	tempDir := t.TempDir()

	// Create source file
	srcPath := filepath.Join(tempDir, "source.txt")
	testContent := "Test content for move"
	if err := os.WriteFile(srcPath, []byte(testContent), 0644); err != nil {
		t.Fatalf("Failed to create source file: %v", err)
	}

	// Test moving file
	destPath := filepath.Join(tempDir, "destination.txt")
	if err := MovePath(srcPath, destPath); err != nil {
		t.Fatalf("MovePath failed: %v", err)
	}

	// Verify source no longer exists
	if PathExists(srcPath) != PathNotFound {
		t.Error("Source file should have been removed")
	}

	// Verify destination exists with correct content
	content, err := os.ReadFile(destPath)
	if err != nil {
		t.Fatalf("Failed to read destination file: %v", err)
	}

	if string(content) != testContent {
		t.Errorf("Content mismatch: got %q, want %q", string(content), testContent)
	}
}

func TestMovePathDirectory(t *testing.T) {
	tempDir := t.TempDir()

	// Create source directory with files
	srcDir := filepath.Join(tempDir, "sourcedir")
	if err := os.MkdirAll(srcDir, 0755); err != nil {
		t.Fatalf("Failed to create source directory: %v", err)
	}

	testFile := filepath.Join(srcDir, "test.txt")
	if err := os.WriteFile(testFile, []byte("test"), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	// Test moving directory
	destDir := filepath.Join(tempDir, "destdir")
	if err := MovePath(srcDir, destDir); err != nil {
		t.Fatalf("MovePath (directory) failed: %v", err)
	}

	// Verify source no longer exists
	if PathExists(srcDir) != PathNotFound {
		t.Error("Source directory should have been removed")
	}

	// Verify destination exists
	if PathExists(destDir) != PathDirectory {
		t.Error("Destination directory should exist")
	}

	// Verify file in destination
	destFile := filepath.Join(destDir, "test.txt")
	if PathExists(destFile) != PathFile {
		t.Error("File should exist in destination directory")
	}
}
