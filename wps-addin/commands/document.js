/**
 * Document management commands.
 */

Protocol.register('document.open', function (params) {
  var path = params.path;
  if (!path) throw new Error('Missing required parameter: path');
  var doc = wps.Application.Documents.Open(path);
  return {
    success: true,
    name: doc.Name,
    path: doc.FullName,
  };
});

Protocol.register('document.save', function () {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  doc.Save();
  return { success: true, name: doc.Name };
});

Protocol.register('document.saveAs', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var path = params.path;
  if (!path) throw new Error('Missing required parameter: path');
  doc.SaveAs2(path);
  return { success: true, name: doc.Name, path: doc.FullName };
});

Protocol.register('document.close', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  // wdSaveChanges = -1, wdDoNotSaveChanges = 0, wdPromptToSaveChanges = -2
  var saveFlag = (params.save !== false) ? -1 : 0;
  doc.Close(saveFlag);
  return { success: true };
});

Protocol.register('document.getInfo', function () {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  return {
    name: doc.Name,
    path: doc.FullName,
    saved: doc.Saved,
    pageCount: doc.ComputeStatistics(2), // wdStatisticPages = 2
    wordCount: doc.ComputeStatistics(0), // wdStatisticWords = 0
    charCount: doc.ComputeStatistics(3), // wdStatisticCharacters = 3
  };
});

Protocol.register('document.getContent', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  var maxLength = params.maxLength || 100000;
  var text = doc.Content.Text || '';
  if (text.length > maxLength) {
    text = text.substring(0, maxLength);
  }
  return {
    text: text,
    truncated: text.length >= maxLength,
    totalLength: doc.Content.Text.length,
  };
});
