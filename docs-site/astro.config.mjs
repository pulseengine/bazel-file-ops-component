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
			// Sidebar will be auto-generated from folder structure
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
