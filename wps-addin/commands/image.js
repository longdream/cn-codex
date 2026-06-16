/**
 * Image insertion commands.
 */

Protocol.register('image.insert', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var path = params.path;
  if (!path) throw new Error('Missing required parameter: path');

  var range;
  if (params.position !== undefined) {
    range = doc.Range(params.position, params.position);
  } else {
    range = wps.Application.Selection.Range;
  }

  var linkToFile = params.linkToFile === true;
  var saveWithDocument = params.saveWithDocument !== false;

  var shape = range.InlineShapes.AddPicture(path, linkToFile, saveWithDocument);

  if (params.width) shape.Width = params.width;
  if (params.height) shape.Height = params.height;

  return {
    success: true,
    width: shape.Width,
    height: shape.Height,
  };
});
