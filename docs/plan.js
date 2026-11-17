// Renders docs/port-plan.md in the browser, so the plan lives in one file and GitHub Pages still shows it
// formatted. Deliberately small: headings, tables, lists, fences, bold, inline code, links. Nothing else.

const escapeHtml = (text) =>
  text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function inline(text) {
  // Inline code first: its content must not be touched by the other rules.
  const parts = [];
  let rest = text.replace(/`([^`]+)`/g, (_, code) => {
    parts.push(`<code>${escapeHtml(code)}</code>`);
    return `\u0000${parts.length - 1}\u0000`;
  });
  rest = escapeHtml(rest);
  rest = rest.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  rest = rest.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>');
  return rest.replace(/\u0000(\d+)\u0000/g, (_, index) => parts[Number(index)]);
}

function render(markdown) {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const out = [];
  let list = null;      // 'ul' | 'ol' | null
  let paragraph = [];

  const closeList = () => {
    if (list) {
      out.push(`</${list}>`);
      list = null;
    }
  };
  const flushParagraph = () => {
    if (paragraph.length) {
      out.push(`<p>${inline(paragraph.join(' '))}</p>`);
      paragraph = [];
    }
  };
  const openList = (kind) => {
    if (list !== kind) {
      closeList();
      out.push(`<${kind}>`);
      list = kind;
    }
  };

  for (let line = 0; line < lines.length; line++) {
    const text = lines[line];

    if (text.startsWith('```')) {
      flushParagraph();
      closeList();
      const body = [];
      line++;
      while (line < lines.length && !lines[line].startsWith('```')) body.push(lines[line++]);
      out.push(`<pre><code>${escapeHtml(body.join('\n'))}</code></pre>`);
      continue;
    }

    if (!text.trim()) {
      flushParagraph();
      closeList();
      continue;
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(text);
    if (heading) {
      flushParagraph();
      closeList();
      const level = heading[1].length;
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }

    // A table: header row, separator row, then rows until a blank line.
    if (text.trim().startsWith('|') && /^\s*\|[\s:|-]+\|\s*$/.test(lines[line + 1] ?? '')) {
      flushParagraph();
      closeList();
      const cells = (row) => row.trim().replace(/^\||\|$/g, '').split('|').map((cell) => cell.trim());
      const header = cells(text);
      const rows = [];
      line += 2;
      while (line < lines.length && lines[line].trim().startsWith('|')) rows.push(cells(lines[line++]));
      line--;
      out.push('<div class="table"><table>');
      out.push(`<thead><tr>${header.map((cell) => `<th>${inline(cell)}</th>`).join('')}</tr></thead>`);
      out.push('<tbody>');
      for (const row of rows) out.push(`<tr>${row.map((cell) => `<td>${inline(cell)}</td>`).join('')}</tr>`);
      out.push('</tbody></table></div>');
      continue;
    }

    const bullet = /^\s*[-*]\s+(.*)$/.exec(text);
    if (bullet) {
      flushParagraph();
      openList('ul');
      out.push(`<li>${inline(bullet[1])}</li>`);
      continue;
    }

    const numbered = /^\s*\d+\.\s+(.*)$/.exec(text);
    if (numbered) {
      flushParagraph();
      openList('ol');
      out.push(`<li>${inline(numbered[1])}</li>`);
      continue;
    }

    closeList();
    paragraph.push(text.trim());
  }

  flushParagraph();
  closeList();
  return out.join('\n');
}

fetch('port-plan.md')
  .then((response) => {
    if (!response.ok) throw new Error(`port-plan.md: ${response.status}`);
    return response.text();
  })
  .then((markdown) => {
    document.getElementById('plan').innerHTML = render(markdown);
  })
  .catch((error) => {
    document.getElementById('plan').innerHTML =
      `<p class="error">The plan did not load: ${escapeHtml(error.message)}. ` +
      `Read it raw at <a href="port-plan.md">port-plan.md</a>.</p>`;
  });
