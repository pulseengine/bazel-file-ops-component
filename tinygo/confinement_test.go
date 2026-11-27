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

func TestJsonMoveSourceStaysInWorkspace(t *testing.T) {
	tempDir := t.TempDir()
	workspaceDir := filepath.Join(tempDir, "ws")
	outside := filepath.Join(tempDir, "outside.txt")
	if err := os.WriteFile(outside, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}

	err := runConfig(t, workspaceDir, Operation{Type: "move_path", SrcPath: outside, DestPath: "moved.txt"})
	if err == nil {
		t.Error("move_path with a source outside the workspace succeeded")
	}
	if PathExists(outside) != PathFile {
		t.Fatal("move_path removed a source outside the workspace")
	}

	inside := filepath.Join(workspaceDir, "in.txt")
	if err := runConfig(t, workspaceDir,
		Operation{Type: "write_file", Path: "in.txt", Content: "x"},
		Operation{Type: "move_path", SrcPath: inside, DestPath: "moved.txt"},
	); err != nil {
		t.Fatalf("move_path inside the workspace rejected: %v", err)
	}
	if PathExists(filepath.Join(workspaceDir, "moved.txt")) != PathFile || PathExists(inside) != PathNotFound {
		t.Error("move_path inside the workspace did not move the file")
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

// GHSA-hm34-86h8-vwx2: a symlink followed by ".." in a raw path.
func TestMoveSourceSymlinkDotDotRefused(t *testing.T) {
	tmp := t.TempDir()
	ws := filepath.Join(tmp, "ws")
	deep := filepath.Join(tmp, "outside", "deep")
	for _, d := range []string{ws, deep} {
		if err := os.MkdirAll(d, 0750); err != nil {
			t.Fatal(err)
		}
	}
	victim := filepath.Join(tmp, "outside", "x")
	if err := os.WriteFile(victim, []byte("victim"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(deep, filepath.Join(ws, "link")); err != nil {
		t.Fatal(err)
	}
	if err := runConfig(t, ws, Operation{Type: "move_path", SrcPath: ws + "/link/../x", DestPath: "stolen"}); err == nil {
		t.Error("move_path with a symlink followed by .. succeeded")
	}
	if PathExists(victim) != PathFile {
		t.Fatal("move_path removed a file outside the workspace")
	}
}

func TestDotDotInWorkDirAndWorkspaceRefused(t *testing.T) {
	tmp := t.TempDir()
	ws := filepath.Join(tmp, "out", "ws")
	if err := runConfig(t, ws, Operation{Type: "run_command", Command: "echo", WorkDir: ws + "/../ws"}); err == nil {
		t.Error("absolute work_dir with a .. component accepted")
	}
	if err := runConfig(t, filepath.Join(tmp, "a")+"/../ws2", Operation{Type: "mkdir", Path: "d"}); err == nil {
		t.Error("workspace_dir with a .. component accepted")
	}
}

// GHSA-hm34-86h8-vwx2: symlinks already inside a destination tree.
func TestCopyIntoExistingSymlinkRefused(t *testing.T) {
	tmp := t.TempDir()
	ws := filepath.Join(tmp, "ws")
	outside := filepath.Join(tmp, "outside")
	src := filepath.Join(tmp, "src")
	for _, d := range []string{ws, outside, filepath.Join(src, "sub")} {
		if err := os.MkdirAll(d, 0750); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(src, "sub", "evil.txt"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(ws, "sub")); err != nil {
		t.Fatal(err)
	}
	if err := runConfig(t, ws, Operation{Type: "copy_directory_contents", SrcPath: src, DestPath: "."}); err == nil {
		t.Error("directory copy through an existing symlink succeeded")
	}
	if PathExists(filepath.Join(outside, "evil.txt")) != PathNotFound {
		t.Fatal("directory copy wrote outside the workspace")
	}
}

func TestWriteThroughExistingFileSymlinkRefused(t *testing.T) {
	tmp := t.TempDir()
	ws := filepath.Join(tmp, "ws")
	if err := os.MkdirAll(ws, 0750); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(tmp, "target.txt")
	if err := os.WriteFile(target, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, filepath.Join(ws, "f.txt")); err != nil {
		t.Fatal(err)
	}
	for _, op := range []Operation{
		{Type: "write_file", Path: "f.txt", Content: "x"},
		{Type: "append_to_file", Path: "f.txt", Content: "x"},
	} {
		_ = runConfig(t, ws, op)
	}
	modDir := filepath.Join(tmp, "mod")
	if err := os.MkdirAll(modDir, 0750); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, filepath.Join(modDir, "go.mod")); err != nil {
		t.Fatal(err)
	}
	if err := SetupGoModule(GoModuleConfig{ModuleName: "m", GoVersion: "1.22"}, modDir); err == nil {
		t.Error("go.mod written through a symlink")
	}
	got, err := os.ReadFile(target)
	if err != nil || string(got) != "keep" {
		t.Fatalf("file outside the workspace changed through a symlink: %q", got)
	}
}
