// Package main provides the TinyGo WebAssembly component entry point
// for the file operations component with WIT interface bindings.
package main

import (
	"encoding/json"
	"fmt"
	"os"
)

// main function for CLI usage during development and testing
func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	operation := os.Args[1]

	// Auto-detect JSON config file (for bootstrap compatibility)
	// If first argument is a file path, treat it as JSON config
	if isJSONConfigFile(operation) {
		handleProcessJsonConfigDirect(operation)
		return
	}

	switch operation {
	case "copy_file":
		handleCopyFile()
	case "copy_directory":
		handleCopyDirectory()
	case "create_directory":
		handleCreateDirectory()
	case "process_json_config":
		handleProcessJsonConfig()
	case "prepare_workspace":
		handlePrepareWorkspace()
	default:
		fmt.Fprintf(os.Stderr, "Unknown operation: %s\n", operation)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Println("TinyGo File Operations Component")
	fmt.Println("Usage: file_ops <operation> [args...]")
	fmt.Println()
	fmt.Println("Operations:")
	fmt.Println("  copy_file --src <src> --dest <dest>")
	fmt.Println("  copy_directory --src <src> --dest <dest>")
	fmt.Println("  create_directory --path <path>")
	fmt.Println("  process_json_config --config <config_file>")
	fmt.Println("  prepare_workspace --config <workspace_config>")
}

func handleCopyFile() {
	src, dest, err := parseCopyArgs(os.Args[2:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing arguments: %v\n", err)
		os.Exit(1)
	}

	if err := CopyFile(src, dest); err != nil {
		fmt.Fprintf(os.Stderr, "Error copying file: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Successfully copied %s to %s\n", src, dest)
}

func handleCopyDirectory() {
	src, dest, err := parseCopyArgs(os.Args[2:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing arguments: %v\n", err)
		os.Exit(1)
	}

	if err := CopyDirectory(src, dest); err != nil {
		fmt.Fprintf(os.Stderr, "Error copying directory: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Successfully copied directory %s to %s\n", src, dest)
}

func handleCreateDirectory() {
	path, err := parsePathArg(os.Args[2:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing arguments: %v\n", err)
		os.Exit(1)
	}

	if err := CreateDirectory(path); err != nil {
		fmt.Fprintf(os.Stderr, "Error creating directory: %v\n", err)
		os.Exit(1)
	}

	fmt.Printf("Successfully created directory %s\n", path)
}

func handleProcessJsonConfig() {
	configFile, err := parseConfigArg(os.Args[2:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing arguments: %v\n", err)
		os.Exit(1)
	}

	configContent, err := os.ReadFile(configFile)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error reading config file: %v\n", err)
		os.Exit(1)
	}

	result, err := ProcessJsonConfig(string(configContent))
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error processing JSON config: %v\n", err)
		os.Exit(1)
	}

	fmt.Println("JSON config processed successfully:")
	fmt.Printf("  Workspace: %s\n", result.WorkspacePath)
	fmt.Printf("  Files: %d\n", len(result.PreparedFiles))
	fmt.Printf("  Time: %d ms\n", result.PreparationTimeMs)
}

func handlePrepareWorkspace() {
	configFile, err := parseConfigArg(os.Args[2:])
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing arguments: %v\n", err)
		os.Exit(1)
	}

	configContent, err := os.ReadFile(configFile)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error reading config file: %v\n", err)
		os.Exit(1)
	}

	var config WorkspaceConfig
	if err := json.Unmarshal(configContent, &config); err != nil {
		fmt.Fprintf(os.Stderr, "Error parsing workspace config: %v\n", err)
		os.Exit(1)
	}

	result, err := PrepareWorkspace(config)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error preparing workspace: %v\n", err)
		os.Exit(1)
	}

	fmt.Println("Workspace prepared successfully:")
	fmt.Printf("  Path: %s\n", result.WorkspacePath)
	fmt.Printf("  Files: %d\n", len(result.PreparedFiles))
	fmt.Printf("  Message: %s\n", result.Message)
	fmt.Printf("  Time: %d ms\n", result.PreparationTimeMs)
}

// Helper functions for argument parsing and JSON detection

// isJSONConfigFile checks if the given path is likely a JSON config file
// This enables bootstrap compatibility where: ./file_ops config.json
func isJSONConfigFile(path string) bool {
	// Check if it ends with .json
	if len(path) > 5 && path[len(path)-5:] == ".json" {
		return true
	}

	// Check if it's an existing file (for paths without .json extension)
	info, err := os.Stat(path)
	if err == nil && !info.IsDir() {
		return true
	}

	return false
}

// handleProcessJsonConfigDirect processes a JSON config file directly from path
// This is used when the file path is provided as the first argument
func handleProcessJsonConfigDirect(configFile string) {
	configContent, err := os.ReadFile(configFile)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error reading config file: %v\n", err)
		os.Exit(1)
	}

	result, err := ProcessJsonConfig(string(configContent))
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error processing JSON config: %v\n", err)
		os.Exit(1)
	}

	fmt.Println("JSON config processed successfully:")
	fmt.Printf("  Workspace: %s\n", result.WorkspacePath)
	fmt.Printf("  Files: %d\n", len(result.PreparedFiles))
	fmt.Printf("  Time: %d ms\n", result.PreparationTimeMs)
}

func parseCopyArgs(args []string) (src, dest string, err error) {
	if len(args) < 4 {
		return "", "", fmt.Errorf("copy operations require --src <src> --dest <dest>")
	}

	for i := 0; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--src":
			src = args[i+1]
		case "--dest":
			dest = args[i+1]
		default:
			return "", "", fmt.Errorf("unknown argument: %s", args[i])
		}
	}

	if src == "" {
		return "", "", fmt.Errorf("--src is required")
	}
	if dest == "" {
		return "", "", fmt.Errorf("--dest is required")
	}

	return src, dest, nil
}

func parsePathArg(args []string) (string, error) {
	if len(args) < 2 || args[0] != "--path" {
		return "", fmt.Errorf("expected --path <path>")
	}
	return args[1], nil
}

func parseConfigArg(args []string) (string, error) {
	if len(args) < 2 || args[0] != "--config" {
		return "", fmt.Errorf("expected --config <config_file>")
	}
	return args[1], nil
}
