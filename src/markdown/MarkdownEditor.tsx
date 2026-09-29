import React from "react";
import EasyMDE from "easymde";
import DOMPurify from "dompurify";

/** Shared EasyMDE surface. Cleanup never commits; callers own draft persistence. */
export function MarkdownEditor({
  value,
  onBlur,
  onChange,
  placeholder = "Write the injected agent instruction in Markdown...",
  minHeight = "440px",
  maxHeight = "620px",
  height = "560px",
  disabled = false,
  startInPreview = false,
}: {
  value: string;
  onBlur: (value: string) => void;
  onChange: (value: string) => void;
  placeholder?: string;
  minHeight?: string;
  maxHeight?: string;
  height?: string;
  disabled?: boolean;
  /** Open on the rendered preview; the toolbar Preview button toggles back to the source. */
  startInPreview?: boolean;
}) {
  const textareaRef = React.useRef<HTMLTextAreaElement>(null);
  const editorRef = React.useRef<EasyMDE | null>(null);
  const syncing = React.useRef(false);
  const onBlurRef = React.useRef(onBlur);
  const onChangeRef = React.useRef(onChange);
  onChangeRef.current = onChange;

  React.useEffect(() => {
    onBlurRef.current = onBlur;
  }, [onBlur]);

  React.useEffect(() => {
    if (!textareaRef.current) {
      return;
    }

    const editor = new EasyMDE({
      autoDownloadFontAwesome: false,
      autofocus: false,
      autoRefresh: { delay: 300 },
      autosave: {
        enabled: false,
        uniqueId: "adashi-rule-editor-disabled",
      },
      element: textareaRef.current,
      forceSync: true,
      initialValue: value,
      lineNumbers: false,
      lineWrapping: true,
      // maxHeight is deliberately not forwarded: EasyMDE copies it onto the scroller as an inline
      // height, which clipped text below a fixed-height host with no way to scroll to it. The host
      // height plus the scroller CSS own the box, so overflowing text scrolls inside it instead.
      minHeight,
      nativeSpellcheck: true,
      placeholder,
      previewImagesInEditor: false,
      renderingConfig: { sanitizerFunction: html => DOMPurify.sanitize(html, { USE_PROFILES: { html: true }, FORBID_TAGS: ["img", "style", "form", "input", "button"], FORBID_ATTR: ["style"], ALLOWED_URI_REGEXP: /^(https?:|mailto:|#)/i }) },
      promptURLs: false,
      sideBySideFullscreen: false,
      spellChecker: false,
      status: false,
      styleSelectedText: false,
      toolbar: ([
        { name: "heading-1", action: EasyMDE.toggleHeading1, className: "adashi-mde-heading", title: "Heading", text: "H1" },
        { name: "bold", action: EasyMDE.toggleBold, className: "adashi-mde-bold", title: "Bold", text: "B" },
        { name: "italic", action: EasyMDE.toggleItalic, className: "adashi-mde-italic", title: "Italic", text: "I" },
        "|",
        { name: "quote", action: EasyMDE.toggleBlockquote, className: "adashi-mde-quote", title: "Quote", text: ">" },
        { name: "unordered-list", action: EasyMDE.toggleUnorderedList, className: "adashi-mde-list", title: "Bullet list", text: "- list" },
        { name: "ordered-list", action: EasyMDE.toggleOrderedList, className: "adashi-mde-ordered", title: "Numbered list", text: "1. list" },
        "|",
        { name: "code", action: EasyMDE.toggleCodeBlock, className: "adashi-mde-code", title: "Code block", text: "{ }" },
        { name: "link", action: EasyMDE.drawLink, className: "adashi-mde-link", title: "Link", text: "link" },
        "|",
        { name: "preview", action: EasyMDE.togglePreview, className: "adashi-mde-preview no-disable", title: "Preview", text: "Preview", noDisable: true },
      ] as EasyMDE.Options["toolbar"]),
      toolbarTips: true,
      uploadImage: false,
    });

    function commit() {
      onBlurRef.current(editor.value());
    }

    const changed = () => { if (!syncing.current) onChangeRef.current(editor.value()); };
    editor.codemirror.on("change", changed);
    editor.codemirror.on("blur", commit);
    editor.codemirror.getInputField().setAttribute("aria-label", "Markdown content");
    editor.codemirror.getInputField().setAttribute("title", "Edit Markdown content; use the formatting toolbar or Preview");
    editorRef.current = editor;

    window.requestAnimationFrame(() => {
      // The toolbar Preview button already reflects the active state that togglePreview sets.
      if (startInPreview) EasyMDE.togglePreview(editor);
      editor.codemirror.refresh();
    });

    return () => {
      editor.codemirror.off("blur", commit);
      editor.codemirror.off("change", changed);
      editor.toTextArea();
      editorRef.current = null;
    };
  }, []);

  React.useEffect(() => {
    const editor = editorRef.current;
    if (editor && editor.value() !== value) {
      syncing.current = true;
      editor.value(value);
      syncing.current = false;
    }
  }, [value]);

  React.useEffect(() => {
    editorRef.current?.codemirror.setOption("readOnly", disabled ? "nocursor" : false);
  }, [disabled]);

  return (
    <div className="markdown-editor-host" style={{ height, minHeight }} onClick={event => {
      const link = (event.target as Element).closest("a");
      if (!link) return;
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (/^(https?:|mailto:)/i.test(href)) window.open(href, "_blank", "noopener,noreferrer");
    }}>
      <textarea className="markdown-editor-source" ref={textareaRef} defaultValue={value} />
    </div>
  );
}
