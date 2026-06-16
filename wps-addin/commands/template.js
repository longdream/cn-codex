/**
 * Template filling commands.
 *
 * Replaces placeholder markers in the document (e.g. {{name}}, {{date}})
 * with the provided values.
 */

Protocol.register('template.fill', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var fields = params.fields;
  if (!fields || typeof fields !== 'object') {
    throw new Error('Missing required parameter: fields (object of key-value pairs)');
  }

  var replacedCount = 0;
  var keys = Object.keys(fields);

  for (var i = 0; i < keys.length; i++) {
    var key = keys[i];
    var value = String(fields[key]);
    var placeholder = params.prefix
      ? params.prefix + key + (params.suffix || '')
      : '{{' + key + '}}';

    var findObj = doc.Content.Find;
    findObj.ClearFormatting();
    findObj.Replacement.ClearFormatting();
    findObj.Text = placeholder;
    findObj.Replacement.Text = value;
    findObj.Forward = true;
    findObj.Wrap = 1; // wdFindContinue
    findObj.MatchCase = true;

    // wdReplaceAll = 2
    var found = findObj.Execute(
      placeholder, true, false, false, false, false, true, 1, false, value, 2
    );
    if (found) replacedCount++;
  }

  return { success: true, fieldsProcessed: keys.length, fieldsReplaced: replacedCount };
});
