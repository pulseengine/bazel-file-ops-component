// Package main provides WIT interface bindings for the TinyGo WebAssembly component
// This file bridges Go functions to the WIT interface exports
package main

//go:build tinygo.wasm

import (
	"encoding/json"
	"unsafe"
)

// Export WIT interface functions
// These functions are exported to the WebAssembly module and callable via WIT

//export file-operations#copy-file
func exportCopyFile(srcPtr, srcLen, destPtr, destLen uint32) uint32 {
	src := ptrToString(srcPtr, srcLen)
	dest := ptrToString(destPtr, destLen)
	
	if err := CopyFile(src, dest); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export file-operations#copy-directory  
func exportCopyDirectory(srcPtr, srcLen, destPtr, destLen uint32) uint32 {
	src := ptrToString(srcPtr, srcLen)
	dest := ptrToString(destPtr, destLen)
	
	if err := CopyDirectory(src, dest); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export file-operations#create-directory
func exportCreateDirectory(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	
	if err := CreateDirectory(path); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export file-operations#remove-path
func exportRemovePath(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	
	if err := RemovePath(path); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export file-operations#path-exists
func exportPathExists(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	return uint32(PathExists(path))
}

//export file-operations#resolve-absolute-path
func exportResolveAbsolutePath(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	
	absPath, err := ResolveAbsolutePath(path)
	if err != nil {
		return encodeError(err.Error())
	}
	
	return encodeString(absPath)
}

//export file-operations#join-paths
func exportJoinPaths(pathsPtr, pathsLen uint32) uint32 {
	// For simplicity, assume paths are JSON-encoded array
	pathsJson := ptrToString(pathsPtr, pathsLen)
	
	var paths []string
	if err := json.Unmarshal([]byte(pathsJson), &paths); err != nil {
		return encodeError(err.Error())
	}
	
	result := JoinPaths(paths)
	return encodeString(result)
}

//export file-operations#get-dirname
func exportGetDirname(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	result := GetDirname(path)
	return encodeString(result)
}

//export file-operations#get-basename
func exportGetBasename(pathPtr, pathLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	result := GetBasename(path)
	return encodeString(result)
}

//export file-operations#list-directory
func exportListDirectory(dirPtr, dirLen, patternPtr, patternLen uint32) uint32 {
	dir := ptrToString(dirPtr, dirLen)
	
	var pattern *string
	if patternLen > 0 {
		p := ptrToString(patternPtr, patternLen)
		pattern = &p
	}
	
	files, err := ListDirectory(dir, pattern)
	if err != nil {
		return encodeError(err.Error())
	}
	
	// Encode as JSON array
	filesJson, err := json.Marshal(files)
	if err != nil {
		return encodeError(err.Error())
	}
	
	return encodeString(string(filesJson))
}

//export file-operations#validate-path
func exportValidatePath(pathPtr, pathLen, allowedDirsPtr, allowedDirsLen uint32) uint32 {
	path := ptrToString(pathPtr, pathLen)
	
	var allowedDirs []string
	if allowedDirsLen > 0 {
		allowedDirsJson := ptrToString(allowedDirsPtr, allowedDirsLen)
		if err := json.Unmarshal([]byte(allowedDirsJson), &allowedDirs); err != nil {
			return encodeError(err.Error())
		}
	}
	
	if err := ValidatePath(path, allowedDirs); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

// JSON Batch Operations Interface

//export json-batch-operations#process-json-config
func exportProcessJsonConfig(configPtr, configLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	
	result, err := ProcessJsonConfig(configJson)
	if err != nil {
		return encodeError(err.Error())
	}
	
	resultJson, err := json.Marshal(result)
	if err != nil {
		return encodeError(err.Error())
	}
	
	return encodeString(string(resultJson))
}

//export json-batch-operations#validate-json-config
func exportValidateJsonConfig(configPtr, configLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	
	if err := ValidateJsonConfig(configJson); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export json-batch-operations#get-json-schema
func exportGetJsonSchema() uint32 {
	schema := GetJsonSchema()
	return encodeString(schema)
}

// Workspace Management Interface

//export workspace-management#prepare-workspace
func exportPrepareWorkspace(configPtr, configLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	
	var config WorkspaceConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return encodeError(err.Error())
	}
	
	result, err := PrepareWorkspace(config)
	if err != nil {
		return encodeError(err.Error())
	}
	
	resultJson, err := json.Marshal(result)
	if err != nil {
		return encodeError(err.Error())
	}
	
	return encodeString(string(resultJson))
}

//export workspace-management#copy-sources
func exportCopySources(sourcesPtr, sourcesLen, destDirPtr, destDirLen uint32) uint32 {
	sourcesJson := ptrToString(sourcesPtr, sourcesLen)
	destDir := ptrToString(destDirPtr, destDirLen)
	
	var sources []FileSpec
	if err := json.Unmarshal([]byte(sourcesJson), &sources); err != nil {
		return encodeError(err.Error())
	}
	
	if err := CopySources(sources, destDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export workspace-management#copy-headers
func exportCopyHeaders(headersPtr, headersLen, destDirPtr, destDirLen uint32) uint32 {
	headersJson := ptrToString(headersPtr, headersLen)
	destDir := ptrToString(destDirPtr, destDirLen)
	
	var headers []FileSpec
	if err := json.Unmarshal([]byte(headersJson), &headers); err != nil {
		return encodeError(err.Error())
	}
	
	if err := CopyHeaders(headers, destDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export workspace-management#copy-bindings
func exportCopyBindings(bindingsDirPtr, bindingsDirLen, destDirPtr, destDirLen uint32) uint32 {
	bindingsDir := ptrToString(bindingsDirPtr, bindingsDirLen)
	destDir := ptrToString(destDirPtr, destDirLen)
	
	if err := CopyBindings(bindingsDir, destDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export workspace-management#setup-package-json
func exportSetupPackageJson(configPtr, configLen, workDirPtr, workDirLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	workDir := ptrToString(workDirPtr, workDirLen)
	
	var config PackageConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return encodeError(err.Error())
	}
	
	if err := SetupPackageJson(config, workDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export workspace-management#setup-go-module
func exportSetupGoModule(configPtr, configLen, workDirPtr, workDirLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	workDir := ptrToString(workDirPtr, workDirLen)
	
	var config GoModuleConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return encodeError(err.Error())
	}
	
	if err := SetupGoModule(config, workDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export workspace-management#setup-cpp-workspace
func exportSetupCppWorkspace(configPtr, configLen, workDirPtr, workDirLen uint32) uint32 {
	configJson := ptrToString(configPtr, configLen)
	workDir := ptrToString(workDirPtr, workDirLen)
	
	var config CppWorkspaceConfig
	if err := json.Unmarshal([]byte(configJson), &config); err != nil {
		return encodeError(err.Error())
	}
	
	if err := SetupCppWorkspace(config, workDir); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

// Security Operations Interface

//export security-operations#configure-preopen-dirs
func exportConfigurePreopenDirs(configsPtr, configsLen uint32) uint32 {
	configsJson := ptrToString(configsPtr, configsLen)
	
	var configs []PreopenDirConfig
	if err := json.Unmarshal([]byte(configsJson), &configs); err != nil {
		return encodeError(err.Error())
	}
	
	if err := ConfigurePreopenDirs(configs); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export security-operations#validate-operation
func exportValidateOperation(operationPtr, operationLen, pathsPtr, pathsLen uint32) uint32 {
	operation := ptrToString(operationPtr, operationLen)
	pathsJson := ptrToString(pathsPtr, pathsLen)
	
	var paths []string
	if err := json.Unmarshal([]byte(pathsJson), &paths); err != nil {
		return encodeError(err.Error())
	}
	
	if err := ValidateOperation(operation, paths); err != nil {
		return encodeError(err.Error())
	}
	return 0 // Success
}

//export security-operations#get-security-context
func exportGetSecurityContext() uint32 {
	context := GetSecurityContext()
	
	contextJson, err := json.Marshal(context)
	if err != nil {
		return encodeError(err.Error())
	}
	
	return encodeString(string(contextJson))
}

// Helper functions for WASM memory management

// ptrToString converts a WebAssembly pointer and length to a Go string
func ptrToString(ptr, length uint32) string {
	if length == 0 {
		return ""
	}
	bytes := (*[1 << 30]byte)(unsafe.Pointer(uintptr(ptr)))[:length]
	return string(bytes)
}

// encodeString encodes a string for return to WebAssembly host
func encodeString(s string) uint32 {
	bytes := []byte(s)
	ptr := allocateMemory(uint32(len(bytes)))
	copy((*[1 << 30]byte)(unsafe.Pointer(uintptr(ptr)))[:len(bytes)], bytes)
	return packPtrLen(ptr, uint32(len(bytes)))
}

// encodeError encodes an error string for return to WebAssembly host
func encodeError(errMsg string) uint32 {
	// In a real implementation, this would set an error flag
	// For now, we'll encode the error as a special return value
	return encodeString("ERROR: " + errMsg)
}

// packPtrLen packs pointer and length into a single uint32
func packPtrLen(ptr, length uint32) uint32 {
	return (ptr << 16) | (length & 0xFFFF)
}

// allocateMemory allocates memory in WebAssembly linear memory
func allocateMemory(size uint32) uint32 {
	// In TinyGo, we can use make to allocate and get the pointer
	bytes := make([]byte, size)
	return uint32(uintptr(unsafe.Pointer(&bytes[0])))
}