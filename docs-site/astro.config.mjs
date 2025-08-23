// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import path from 'path';
import { fileURLToPath } from 'url';
import mermaid from 'astro-mermaid';

// https://astro.build/config
export default defineConfig({
	site: 'https://bazel-file-ops.pulseengine.eu',
	vite: {
		resolve: {
			alias: {
				'@components': path.resolve(path.dirname(fileURLToPath(import.meta.url)), './src/components'),
			},
		},
	},
	integrations: [
		mermaid(),
		starlight({
			title: 'Bazel File Operations Component',
			description: 'Universal file operations for Bazel build systems via WebAssembly components',
			expressiveCode: {
				themes: ['github-dark', 'github-light'],
				// Use Python grammar for Starlark since Starlark syntax is a subset of Python
				shiki: {
					langAlias: {
						'starlark': 'python',
						'star': 'python',
						'bzl': 'python',
						'bazel': 'python'
					}
				}
			},
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/pulseengine/bazel-file-ops-component',
				},
			],
			editLink: {
				baseUrl: 'https://github.com/pulseengine/bazel-file-ops-component/edit/main/docs-site/',
			},
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Overview', slug: 'index' },
						{ label: 'Quick Start', slug: 'getting-started' },
						{ label: 'Installation', slug: 'installation' },
						{ label: 'First Operations', slug: 'first-operations' },
					],
				},
				{
					label: 'Architecture',
					items: [
						{ label: 'System Overview', slug: 'architecture/overview' },
						{ label: 'Security Model', slug: 'architecture/security-model' },
						{ label: 'Dual Implementation', slug: 'architecture/dual-implementation' },
						{ label: 'WebAssembly Sandboxing', slug: 'architecture/wasm-sandboxing' },
					],
				},
				{
					label: 'Usage',
					items: [
						{ label: 'JSON Batch Processing', slug: 'usage/json-batch' },
						{ label: 'Individual Operations', slug: 'usage/individual-ops' },
						{ label: 'Bazel Integration', slug: 'usage/bazel-integration' },
						{ label: 'Security Configuration', slug: 'usage/security-config' },
					],
				},
				{
					label: 'Implementations',
					items: [
						{ label: 'TinyGo Component', slug: 'implementations/tinygo' },
						{ label: 'Rust Component', slug: 'implementations/rust' },
						{ label: 'Performance Comparison', slug: 'implementations/comparison' },
						{ label: 'Selection Guide', slug: 'implementations/selection' },
					],
				},
				{
					label: 'Examples',
					items: [
						{ label: 'Basic Usage', slug: 'examples/basic-usage' },
						{ label: 'Workspace Setup', slug: 'examples/workspace-setup' },
						{ label: 'C++ Integration', slug: 'examples/cpp-integration' },
						{ label: 'Go Integration', slug: 'examples/go-integration' },
						{ label: 'Advanced Scenarios', slug: 'examples/advanced' },
					],
				},
				{
					label: 'Integration',
					items: [
						{ label: 'rules_wasm_component', slug: 'integration/rules-wasm-component' },
						{ label: 'rules_rust', slug: 'integration/rules-rust' },
						{ label: 'rules_go', slug: 'integration/rules-go' },
						{ label: 'rules_cc', slug: 'integration/rules-cc' },
						{ label: 'Custom Rule Sets', slug: 'integration/custom-rules' },
					],
				},
				{
					label: 'Security',
					items: [
						{ label: 'WASM Sandbox', slug: 'security/wasm-sandbox' },
						{ label: 'Preopen Directories', slug: 'security/preopen-dirs' },
						{ label: 'Capability Security', slug: 'security/capability-security' },
						{ label: 'Security Levels', slug: 'security/levels' },
					],
				},
				{
					label: 'Deployment',
					items: [
						{ label: 'OCI Registry', slug: 'deployment/oci-registry' },
						{ label: 'GitHub Releases', slug: 'deployment/github-releases' },
						{ label: 'Versioning', slug: 'deployment/versioning' },
						{ label: 'Distribution', slug: 'deployment/distribution' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'WIT Interface', slug: 'reference/wit-interface' },
						{ label: 'JSON Schema', slug: 'reference/json-schema' },
						{ label: 'API Reference', slug: 'reference/api' },
						{ label: 'Configuration', slug: 'reference/config' },
					],
				},
				{
					label: 'Troubleshooting',
					items: [
						{ label: 'Common Issues', slug: 'troubleshooting/common-issues' },
						{ label: 'Performance Issues', slug: 'troubleshooting/performance' },
						{ label: 'Security Errors', slug: 'troubleshooting/security' },
						{ label: 'Debugging Guide', slug: 'troubleshooting/debugging' },
					],
				},
			],
			customCss: [
				'./src/styles/custom.css',
			],
			head: [
				{
					tag: 'script',
					content: `
// Diagram modal functionality
document.addEventListener('DOMContentLoaded', function() {
  // Create modal HTML
  const modalHTML = \`
    <div id="diagramModal" class="diagram-modal">
      <span class="modal-close">&times;</span>
      <div id="modalContent"></div>
    </div>
  \`;
  document.body.insertAdjacentHTML('beforeend', modalHTML);

  const modal = document.getElementById('diagramModal');
  const modalContent = document.getElementById('modalContent');
  const closeBtn = document.querySelector('.modal-close');

  function addClickListeners() {
    const diagrams = document.querySelectorAll('svg[id^="mermaid-"]');
    diagrams.forEach(diagram => {
      diagram.style.cursor = 'pointer';
      diagram.addEventListener('click', function() {
        const clone = this.cloneNode(true);
        modalContent.innerHTML = '';
        modalContent.appendChild(clone);
        modal.classList.add('active');
        document.body.style.overflow = 'hidden';
      });
    });
  }

  function closeModal() {
    modal.classList.remove('active');
    document.body.style.overflow = '';
    modalContent.innerHTML = '';
  }

  if (closeBtn) {
    closeBtn.addEventListener('click', closeModal);
  }

  if (modal) {
    modal.addEventListener('click', function(e) {
      if (e.target === modal) closeModal();
    });
  }

  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape' && modal && modal.classList.contains('active')) {
      closeModal();
    }
  });

  addClickListeners();

  // Re-add listeners after navigation
  document.addEventListener('astro:page-load', addClickListeners);
});
					`,
				},
			],
		}),
	],
});