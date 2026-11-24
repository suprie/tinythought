const bodyField = document.querySelector("#content");
const picker = document.querySelector("#image");
const insertButton = document.querySelector("#insert-images");
const status = document.querySelector("#image-status");
const staged = new Map();

function pendingSlots() {
  return new Set(
    Array.from(bodyField.value.matchAll(/\]\(upload-image:(\d+)\)/g), (match) => Number(match[1])),
  );
}

function removeUnusedFiles() {
  const slots = pendingSlots();
  for (const slot of staged.keys()) {
    if (!slots.has(slot)) {
      staged.delete(slot);
    }
  }
}

picker.hidden = true;
document.querySelector("#image-label").hidden = true;
insertButton.hidden = false;
insertButton.addEventListener("click", () => picker.click());

picker.addEventListener("change", () => {
  const files = Array.from(picker.files);
  picker.value = "";
  if (!files.length) {
    return;
  }
  removeUnusedFiles();
  if (staged.size + files.length > 4 || files.some((file) => file.size > 5 * 1024 * 1024)) {
    status.textContent = "Choose up to four images per save, 5 MB each.";
    return;
  }

  const used = pendingSlots();
  const missing = [...used].filter((slot) => !staged.has(slot)).sort((a, b) => a - b);
  const snippets = [];
  let next = Math.max(0, ...used) + 1;
  for (const file of files) {
    const existing = missing.shift();
    const slot = existing ?? next++;
    staged.set(slot, file);
    if (existing === undefined) {
      snippets.push(`![Post image](upload-image:${slot})`);
    }
  }
  if (snippets.length) {
    const caret = bodyField.selectionEnd;
    bodyField.setRangeText(`\n\n${snippets.join("\n\n")}\n\n`, caret, caret, "end");
  }
  status.textContent = `${staged.size} selected. Move the Markdown to position images and edit the text in brackets to describe them.`;
  bodyField.focus();
});

bodyField.form.addEventListener("formdata", (event) => {
  removeUnusedFiles();
  event.formData.delete("image");
  for (const [slot, file] of staged) {
    event.formData.append(`image-${slot}`, file);
  }
});
