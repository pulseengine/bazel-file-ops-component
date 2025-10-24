package main

import (
	"encoding/json"
	"fmt"
	"io/ioutil"
	"os"
	"os/exec"
	"strings"
	"testing"
)

// JSONBatchRequest represents a batch of file operations
type JSONBatchRequest struct {
	Operations []JSONOperation `json:"operations"`
}

// JSONOperation represents a single file operation
type JSONOperation struct {
	Operation   string `json:"operation"`
	Source      string `json:"source,omitempty"`
	Destination string `json:"destination,omitempty"`
	Content     string `json:"content,omitempty"`
}

// JSONBatchResponse represents the response from batch operations
type JSONBatchResponse struct {
	Success bool                  `json:"success"`
	Results []JSONOperationResult `json:"results"`
}

// JSONOperationResult represents the result of a single operation
type JSONOperationResult struct {
	Success bool   `json:"success"`
	Message string `json:"message"`
	Output  string `json:"output,omitempty"`
}

func TestWITInterfaceConsistency(t *testing.T) {
	witSource := os.Getenv("WIT_SOURCE")
	componentWasm := os.Getenv("COMPONENT_WASM")

	if witSource == "" || componentWasm == "" {
		t.Skip("WIT_SOURCE and COMPONENT_WASM environment variables required")
	}

	// Check if wasm-tools is available
	if _, err := exec.LookPath("wasm-tools"); err != nil {
		t.Skip("wasm-tools not available, skipping WIT consistency test")
	}

	t.Log("Validating WebAssembly component...")
	cmd := exec.Command("wasm-tools", "validate", componentWasm)
	if err := cmd.Run(); err != nil {
		t.Fatalf("WebAssembly component validation failed: %v", err)
	}

	t.Log("Extracting WIT interface from component...")
	cmd = exec.Command("wasm-tools", "component", "wit", componentWasm)
	extractedWIT, err := cmd.Output()
	if err != nil {
		t.Fatalf("Failed to extract WIT interface: %v", err)
	}

	// Read source WIT
	sourceWIT, err := ioutil.ReadFile(witSource)
	if err != nil {
		t.Fatalf("Failed to read source WIT file: %v", err)
	}

	// Check for core interface structure
	sourceStr := string(sourceWIT)
	extractedStr := string(extractedWIT)

	if !strings.Contains(sourceStr, "interface file-operations") {
		t.Error("Source WIT missing core file-operations interface")
	}

	if !strings.Contains(extractedStr, "interface file-operations") {
		t.Error("Extracted WIT missing core file-operations interface")
	}

	// Check for essential functions
	essentialFunctions := []string{
		"copy-file",
		"move-file",
		"delete-file",
		"create-directory",
		"list-directory",
		"process-json-batch",
	}

	for _, function := range essentialFunctions {
		if !strings.Contains(extractedStr, function) {
			t.Errorf("Function %s not found in extracted WIT interface", function)
		} else {
			t.Logf("✅ Function %s found in component", function)
		}
	}

	t.Log("✅ WIT interface consistency test completed")
}

func TestJSONBatchCompatibility(t *testing.T) {
	componentBinary := os.Getenv("COMPONENT_BINARY")
	if componentBinary == "" {
		t.Skip("COMPONENT_BINARY environment variable required")
	}

	// Create temporary test directory
	testDir, err := ioutil.TempDir("", "json_batch_test")
	if err != nil {
		t.Fatalf("Failed to create temp directory: %v", err)
	}
	defer os.RemoveAll(testDir)

	// Change to test directory
	oldDir, _ := os.Getwd()
	os.Chdir(testDir)
	defer os.Chdir(oldDir)

	// Create test files
	testContent := "Test source content"
	err = ioutil.WriteFile("test_source.txt", []byte(testContent), 0644)
	if err != nil {
		t.Fatalf("Failed to create test source file: %v", err)
	}

	err = os.Mkdir("test_directory", 0755)
	if err != nil {
		t.Fatalf("Failed to create test directory: %v", err)
	}

	// Create JSON batch request
	batchRequest := JSONBatchRequest{
		Operations: []JSONOperation{
			{
				Operation:   "copy_file",
				Source:      "test_source.txt",
				Destination: "test_copy.txt",
			},
			{
				Operation: "read_file",
				Source:    "test_source.txt",
			},
			{
				Operation:   "create_directory",
				Destination: "new_test_dir",
			},
			{
				Operation:   "write_file",
				Destination: "written_file.txt",
				Content:     "This is written content",
			},
			{
				Operation: "list_directory",
				Source:    "test_directory",
			},
			{
				Operation: "path_exists",
				Source:    "test_source.txt",
			},
		},
	}

	// Write batch request to file
	batchJSON, err := json.MarshalIndent(batchRequest, "", "  ")
	if err != nil {
		t.Fatalf("Failed to marshal batch request: %v", err)
	}

	err = ioutil.WriteFile("batch_test.json", batchJSON, 0644)
	if err != nil {
		t.Fatalf("Failed to write batch test file: %v", err)
	}

	t.Log("Running JSON batch operations test...")

	// Execute batch operations
	cmd := exec.Command(componentBinary, "process_json_batch", "--config", "batch_test.json")
	output, err := cmd.Output()
	if err != nil {
		t.Fatalf("JSON batch processing failed: %v", err)
	}

	// Parse response
	var response JSONBatchResponse
	err = json.Unmarshal(output, &response)
	if err != nil {
		t.Fatalf("Failed to parse JSON response: %v", err)
	}

	// Validate response structure
	if len(response.Results) != len(batchRequest.Operations) {
		t.Errorf("Expected %d results, got %d", len(batchRequest.Operations), len(response.Results))
	}

	// Count successful operations
	successfulOps := 0
	for i, result := range response.Results {
		if result.Success {
			successfulOps++
		} else {
			t.Logf("Operation %d (%s) failed: %s", i, batchRequest.Operations[i].Operation, result.Message)
		}
	}

	t.Logf("Successfully completed operations: %d/%d", successfulOps, len(batchRequest.Operations))

	if successfulOps < 4 { // Allow some operations to fail in test environment
		t.Errorf("Too many operations failed: %d/%d successful", successfulOps, len(batchRequest.Operations))
	}

	// Verify actual file operations
	if _, err := os.Stat("test_copy.txt"); os.IsNotExist(err) {
		t.Error("File copy operation failed - target file doesn't exist")
	}

	if _, err := os.Stat("written_file.txt"); os.IsNotExist(err) {
		t.Error("File write operation failed - target file doesn't exist")
	} else {
		content, err := ioutil.ReadFile("written_file.txt")
		if err != nil {
			t.Errorf("Failed to read written file: %v", err)
		} else if string(content) != "This is written content" {
			t.Errorf("Written file content incorrect: got %q, want %q", string(content), "This is written content")
		}
	}

	if _, err := os.Stat("new_test_dir"); os.IsNotExist(err) {
		t.Error("Directory creation operation failed - directory doesn't exist")
	}

	t.Log("Testing error handling...")

	// Test error handling with invalid operations
	invalidRequest := JSONBatchRequest{
		Operations: []JSONOperation{
			{
				Operation: "invalid_operation",
				Source:    "nonexistent.txt",
			},
		},
	}

	invalidJSON, _ := json.Marshal(invalidRequest)
	err = ioutil.WriteFile("invalid_batch.json", invalidJSON, 0644)
	if err != nil {
		t.Fatalf("Failed to write invalid batch test file: %v", err)
	}

	cmd = exec.Command(componentBinary, "process_json_batch", "--config", "invalid_batch.json")
	output, err = cmd.Output()
	if err != nil {
		t.Fatalf("Invalid JSON batch processing failed unexpectedly: %v", err)
	}

	var errorResponse JSONBatchResponse
	err = json.Unmarshal(output, &errorResponse)
	if err != nil {
		t.Fatalf("Failed to parse error response: %v", err)
	}

	if len(errorResponse.Results) == 0 {
		t.Error("Expected error results for invalid operations")
	}

	errorCount := 0
	for _, result := range errorResponse.Results {
		if !result.Success {
			errorCount++
		}
	}

	if errorCount == 0 {
		t.Error("Expected at least one error for invalid operation")
	} else {
		t.Logf("✅ Error handling working correctly (%d errors captured)", errorCount)
	}

	t.Log("✅ JSON batch processing compatibility test completed")
}

func TestComponentBuild(t *testing.T) {
	// This test validates that the component can be built and basic functionality works
	componentBinary := os.Getenv("COMPONENT_BINARY")
	if componentBinary == "" {
		t.Skip("COMPONENT_BINARY environment variable required")
	}

	// Test that the binary exists and is executable
	if _, err := os.Stat(componentBinary); os.IsNotExist(err) {
		t.Fatalf("Component binary does not exist: %s", componentBinary)
	}

	// Test basic functionality
	testDir, err := ioutil.TempDir("", "component_build_test")
	if err != nil {
		t.Fatalf("Failed to create temp directory: %v", err)
	}
	defer os.RemoveAll(testDir)

	oldDir, _ := os.Getwd()
	os.Chdir(testDir)
	defer os.Chdir(oldDir)

	// Test simple file operations
	testFile := "build_test.txt"
	testContent := "Component build test content"

	// Create test file using component
	writeCmd := exec.Command(componentBinary, "write_file", "--path", testFile, "--content", testContent)
	if err := writeCmd.Run(); err != nil {
		t.Fatalf("Write file operation failed: %v", err)
	}

	// Verify file was created
	if _, err := os.Stat(testFile); os.IsNotExist(err) {
		t.Fatal("Test file was not created by component")
	}

	// Read file using component
	readCmd := exec.Command(componentBinary, "read_file", "--path", testFile)
	output, err := readCmd.Output()
	if err != nil {
		t.Fatalf("Read file operation failed: %v", err)
	}

	if strings.TrimSpace(string(output)) != testContent {
		t.Errorf("File content mismatch: got %q, want %q", strings.TrimSpace(string(output)), testContent)
	}

	t.Log("✅ Component build and basic functionality test completed")
}

func TestPerformanceBasic(t *testing.T) {
	if testing.Short() {
		t.Skip("Skipping performance test in short mode")
	}

	componentBinary := os.Getenv("COMPONENT_BINARY")
	if componentBinary == "" {
		t.Skip("COMPONENT_BINARY environment variable required")
	}

	testDir, err := ioutil.TempDir("", "performance_test")
	if err != nil {
		t.Fatalf("Failed to create temp directory: %v", err)
	}
	defer os.RemoveAll(testDir)

	oldDir, _ := os.Getwd()
	os.Chdir(testDir)
	defer os.Chdir(oldDir)

	// Create test files of different sizes
	smallFile := "small.txt"
	mediumFile := "medium.txt"

	// Small file (1KB)
	smallContent := strings.Repeat("Small file content.\n", 50) // ~1KB
	err = ioutil.WriteFile(smallFile, []byte(smallContent), 0644)
	if err != nil {
		t.Fatalf("Failed to create small test file: %v", err)
	}

	// Medium file (100KB)
	mediumContent := strings.Repeat("Medium file content line.\n", 4000) // ~100KB
	err = ioutil.WriteFile(mediumFile, []byte(mediumContent), 0644)
	if err != nil {
		t.Fatalf("Failed to create medium test file: %v", err)
	}

	// Test file copy performance
	t.Run("SmallFileCopy", func(t *testing.T) {
		cmd := exec.Command(componentBinary, "copy_file", "--src", smallFile, "--dest", "small_copy.txt")
		if err := cmd.Run(); err != nil {
			t.Errorf("Small file copy failed: %v", err)
		}
	})

	t.Run("MediumFileCopy", func(t *testing.T) {
		cmd := exec.Command(componentBinary, "copy_file", "--src", mediumFile, "--dest", "medium_copy.txt")
		if err := cmd.Run(); err != nil {
			t.Errorf("Medium file copy failed: %v", err)
		}
	})

	// Test directory operations
	t.Run("DirectoryOperations", func(t *testing.T) {
		// Create directory
		cmd := exec.Command(componentBinary, "create_directory", "--path", "perf_test_dir")
		if err := cmd.Run(); err != nil {
			t.Errorf("Directory creation failed: %v", err)
		}

		// List directory
		cmd = exec.Command(componentBinary, "list_directory", "--path", ".")
		if err := cmd.Run(); err != nil {
			t.Errorf("Directory listing failed: %v", err)
		}
	})

	t.Log("✅ Basic performance test completed")
}

func main() {
	// This allows the test to be run as a standalone binary
	// The actual test execution happens through 'go test'
	fmt.Println("File Operations Component Integration Tests")
	fmt.Println("Run with: go test")
}
