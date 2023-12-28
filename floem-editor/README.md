# Floem Editor

The core editor part of Lapce as a general text editor view.

The primary aim is to allow the editor to be used in other projects, with enough room for extensibility that it can be used in Lapce itself.  
  
There are a variety of features that are in Lapce but do not belong in the core editor. Ex: syntax highlighting, language server integration, the specific way Lapce saves, how Lapce handles keybindings, etc.  
Other features are more ambiguous as to whether they should be within this repo or not. Ex: editor splits, find/replace, etc.  
  
## Usage

There are two main structures for using the editor: `Editor` and `Document`.  
`Document` is a trait which provides various functionality. Think of it like the underlying file. It tracks undo/redo history, the text, phantom text, etcetera.  
`Editor` can be considered the data part of the `View`. It holds `Rc<dyn Document>` within it. This allows one `D: Document` to be shown in multiple editors.  
  
There are several functions for creating the actual `View`. Such as (TODO) `input_view` for single-line text inputs, and `editor_content_view` for a full-fledged editor. The parts making up those views are meant to be composable, so that if you want to change the gutter from `editor_content_view` you can simply declare you own function using most of the same code.

TODO: section on `TextDocument`
TODO: section on common views, like full editor and text input view
TODO: section on customization