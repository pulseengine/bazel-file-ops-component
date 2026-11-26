// Package main: tests for path confinement (destinations inside the
// workspace, run_command directories inside the workspace or its parent).
package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func runConfig(t *testing.T, workspaceDir string, ops ...Operation) error {
	t.Helper()
	configJson, err := json.Marshal(JsonConfig{WorkspaceDir: workspaceDir, Operations: ops})
	if err != nil {
		t.Fatalf("Failed to marshal config: %v", err)
	}
	_, err = ProcessJsonConfig(string(configJson))
	return err
}

func TestIsWithin(t *testing.T) {
	tempDir := t.TempDir()
	root := filepath.Join(tempDir, "work")
	if err := os.MkdirAll(filepath.Join(root, "sub"), 0750); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(tempDir, "outside")
	if err := os.MkdirAll(outside, 0750); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "link")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(tempDir, "missing"), filepath.Join(root, "dangling")); err != nil {
		t.Fatal(err)
	}

	cases := []struct {
		path string
		want bool
	}{
		{root, true},
		{filepath.Join(root, "sub"), true},
		{filepath.Join(root, "sub", "new", "file.txt"), true},
		{filepath.Join(root, "a..b"), true},
		{filepath.Join(tempDir, "work-other"), false},
		{filepath.Join(tempDir, "work-other", "file.txt"), false},
		{tempDir, false},
		{filepath.Join(root, "link", "file.txt"), false},
		{filepath.Join(root, "dangling"), false},
		{filepath.Join(root, "dangling", "file.txt"), false},
	}
	for _, c := range cases {
		if got := isWithin(root, c.path); got != c.want {
			t.Errorf("isWithin(%q, %q) = %v, want %v", root, c.path, got, c.want)
		}
	}
}

func TestContainsPathTraversal(t *testing.T) {
	cases := map[string]bool{
		"../x":      true,
		"a/../../x": true,
		"..":        true,
		"a..b":      false,
		"a/b..c/d":  false,
		"/a/../b":   false, // cleans to /b
		"a/b":       false,
	}
	for p, want := range cases {
		if got := containsPathTraversal(p); got != want {
			t.Errorf("containsPathTraversal(%q) = %v, want %v", p, got, want)
		}
	}
}

func TestValidatePathAllowedDirsBoundary(t *testing.T) {
	tempDir := t.TempDir()
	allowed := filepath.Join(tempDir, "work")
	if err := ValidatePath(filepath.Join(allowed, "f"), []string{allowed}); err != nil {
		t.Errorf("path inside allowed dir rejected: %v", err)
	}
	if err := ValidatePath(filepath.Join(tempDir, "work-other", "f"), []string{allowed}); err == nil {
		t.Error("sibling with a shared name prefix accepted")
	}
}

func TestCopyFileRejectsRelativeTraversalSource(t *testing.T) {
	tempDir := t.TempDir()
	// The source exists, so only validation can stop the copy.
	if err := os.WriteFile(filepath.Join(tempDir, "outside.txt"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	cwd := filepath.Join(tempDir, "cwd")
	if err := os.MkdirAll(cwd, 0750); err != nil {
		t.Fatal(err)
	}
	old, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(cwd); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(old) })

	dest := filepath.Join(tempDir, "dest.txt")
	if err := CopyFile("../outside.txt", dest); err == nil {
		t.Error("CopyFile accepted a source with a .. component")
	}
	if PathExists(dest) != PathNotFound {
		t.Error("CopyFile wrote the destination")
	}
}

func TestJsonDestinationsStayInWorkspace(t *testing.T) {
	tempDir := t.TempDir()
	src := filepath.Join(tempDir, "src.txt")
	if err := os.WriteFile(src, []byte("data"), 0600); err != nil {
		t.Fatal(err)
	}

	ops := map[string]Operation{
		"write_file":        {Type: "write_file", Path: "../escape.txt", Content: "x"},
		"append_to_file":    {Type: "append_to_file", Path: "../escape.txt", Content: "x"},
		"mkdir":             {Type: "mkdir", Path: "../escape.txt"},
		"copy_file":         {Type: "copy_file", SrcPath: src, DestPath: "../escape.txt"},
		"concatenate_files": {Type: "concatenate_files", Sources: []string{src}, DestPath: "../escape.txt"},
		"move_path":         {Type: "move_path", SrcPath: src, DestPath: "../escape.txt"},
	}
	for name, op := range ops {
		t.Run(name, func(t *testing.T) {
			workspaceDir := filepath.Join(tempDir, "ws-"+name)
			if err := runConfig(t, workspaceDir, op); err == nil {
				t.Errorf("%s with dest outside the workspace succeeded", name)
			}
			if PathExists(filepath.Join(tempDir, "escape.txt")) != PathNotFound {
				t.Fatalf("%s wrote outside the workspace", name)
			}
			if PathExists(src) != PathFile {
				t.Fatalf("%s removed the source", name)
			}
		})
	}
}

func TestJsonDestinationThroughSymlinkRejected(t *testing.T) {
	tempDir := t.TempDir()
	workspaceDir := filepath.Join(tempDir, "ws")
	outside := filepath.Join(tempDir, "outside")
	for _, d := range []string{workspaceDir, outside} {
		if err := os.MkdirAll(d, 0750); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Symlink(outside, filepath.Join(workspaceDir, "link")); err != nil {
		t.Fatal(err)
	}

	err := runConfig(t, workspaceDir, Operation{Type: "write_file", Path: "link/f.txt", Content: "x"})
	if err == nil {
		t.Error("write through a symlink out of the workspace succeeded")
	}
	if PathExists(filepath.Join(outside, "f.txt")) != PathNotFound {
		t.Fatal("file written outside the workspace")
	}
}

func TestJsonRunCommandBounds(t *testing.T) {
	tempDir := t.TempDir()
	parent := filepath.Join(tempDir, "out")
	workspaceDir := filepath.Join(parent, "ws")

	// rules_wasm_component's wit_bindgen.bzl pattern: output next to the
	// workspace, in its parent.
	err := runConfig(t, workspaceDir, Operation{
		Type: "run_command", Command: "echo", Args: []string{"ok"},
		WorkDir: ".", OutputFile: "../bindings.txt",
	})
	if err != nil {
		t.Fatalf("output_file in the workspace parent rejected: %v", err)
	}
	if PathExists(filepath.Join(parent, "bindings.txt")) != PathFile {
		t.Error("output_file in the workspace parent not written")
	}

	err = runConfig(t, workspaceDir, Operation{
		Type: "run_command", Command: "echo", Args: []string{"x"},
		OutputFile: "../../escape.txt",
	})
	if err == nil {
		t.Error("output_file two levels up accepted")
	}
	if PathExists(filepath.Join(tempDir, "escape.txt")) != PathNotFound {
		t.Fatal("output written outside the bound")
	}

	if err := runConfig(t, workspaceDir, Operation{
		Type: "run_command", Command: "echo", Args: []string{"x"}, WorkDir: tempDir,
	}); err == nil {
		t.Error("absolute work_dir outside the bound accepted")
	}
}

func TestPrepareWorkspaceDestinationStaysInWorkDir(t *testing.T) {
	tempDir := t.TempDir()
	src := filepath.Join(tempDir, "a.h")
	if err := os.WriteFile(src, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	dest := "../../escape.h"
	_, err := PrepareWorkspace(WorkspaceConfig{
		WorkDir: filepath.Join(tempDir, "ws"),
		Sources: []FileSpec{{Source: src, Destination: &dest}},
	})
	if err == nil {
		t.Error("FileSpec destination outside work_dir accepted")
	}
	if PathExists(filepath.Join(filepath.Dir(tempDir), "escape.h")) != PathNotFound {
		t.Fatal("file written outside work_dir")
	}
}
