/**
 * Text manipulation commands.
 */

Protocol.register('text.insert', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var text = params.text;
  if (text === undefined || text === null) throw new Error('Missing required parameter: text');

  var position = params.position || 'cursor';
  var range;

  if (position === 'end') {
    range = doc.Content;
    range.Collapse(0); // wdCollapseEnd = 0
  } else if (position === 'start') {
    range = doc.Content;
    range.Collapse(1); // wdCollapseStart = 1
  } else if (typeof position === 'number') {
    range = doc.Range(position, position);
  } else {
    // Default: at cursor / selection
    range = wps.Application.Selection.Range;
  }

  range.InsertAfter(text);
  return { success: true, insertedLength: text.length };
});

Protocol.register('text.replace', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var find = params.find;
  var replaceWith = params.replace;
  if (!find) throw new Error('Missing required parameter: find');
  if (replaceWith === undefined) throw new Error('Missing required parameter: replace');

  var replaceAll = params.all !== false;
  var matchCase = params.matchCase === true;

  var findObj = doc.Content.Find;
  findObj.ClearFormatting();
  findObj.Replacement.ClearFormatting();
  findObj.Text = find;
  findObj.Replacement.Text = replaceWith;
  findObj.Forward = true;
  findObj.Wrap = 1; // wdFindContinue
  findObj.MatchCase = matchCase;

  // wdReplaceAll = 2, wdReplaceOne = 1
  var replaceFlag = replaceAll ? 2 : 1;
  var found = findObj.Execute(
    find, matchCase, false, false, false, false, true, 1, false, replaceWith, replaceFlag
  );

  return { success: true, replaced: found };
});

Protocol.register('text.delete', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  if (params.start !== undefined && params.end !== undefined) {
    var range = doc.Range(params.start, params.end);
    range.Delete();
    return { success: true };
  }

  // Delete selected text.
  var sel = wps.Application.Selection;
  if (sel.Type === 0) throw new Error('No selection and no range specified');
  sel.Delete();
  return { success: true };
});
