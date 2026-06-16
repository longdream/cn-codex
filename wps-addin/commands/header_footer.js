/**
 * Header and footer commands.
 */

Protocol.register('header.set', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var text = params.text;
  if (text === undefined) throw new Error('Missing required parameter: text');

  var sectionIndex = params.section || 1;
  if (sectionIndex > doc.Sections.Count) {
    throw new Error('Section index out of range');
  }

  var section = doc.Sections.Item(sectionIndex);
  // wdHeaderFooterPrimary = 1
  var header = section.Headers.Item(1);
  header.Range.Text = text;

  if (params.font) {
    var font = header.Range.Font;
    if (params.font.name) font.Name = params.font.name;
    if (params.font.size) font.Size = params.font.size;
  }

  return { success: true };
});

Protocol.register('footer.set', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var text = params.text;
  if (text === undefined) throw new Error('Missing required parameter: text');

  var sectionIndex = params.section || 1;
  if (sectionIndex > doc.Sections.Count) {
    throw new Error('Section index out of range');
  }

  var section = doc.Sections.Item(sectionIndex);
  // wdHeaderFooterPrimary = 1
  var footer = section.Footers.Item(1);
  footer.Range.Text = text;

  if (params.font) {
    var font = footer.Range.Font;
    if (params.font.name) font.Name = params.font.name;
    if (params.font.size) font.Size = params.font.size;
  }

  return { success: true };
});
