/**
 * Comment and revision tracking commands.
 */

Protocol.register('comment.add', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var text = params.text;
  if (!text) throw new Error('Missing required parameter: text');

  var range;
  if (params.start !== undefined && params.end !== undefined) {
    range = doc.Range(params.start, params.end);
  } else {
    range = wps.Application.Selection.Range;
  }

  var comment = doc.Comments.Add(range, text);
  return { success: true, commentIndex: comment.Index };
});

Protocol.register('comment.list', function () {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  var comments = [];
  var count = doc.Comments.Count;
  for (var i = 1; i <= count; i++) {
    var c = doc.Comments.Item(i);
    comments.push({
      index: c.Index,
      author: c.Author || '',
      text: c.Range.Text || '',
      scope: c.Scope ? c.Scope.Text : '',
      date: c.Date ? c.Date.toString() : '',
    });
  }
  return { comments: comments };
});

Protocol.register('comment.delete', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var index = params.index;
  if (index === undefined) throw new Error('Missing required parameter: index');

  if (index < 1 || index > doc.Comments.Count) {
    throw new Error('Comment index out of range');
  }
  doc.Comments.Item(index).Delete();
  return { success: true };
});

Protocol.register('revision.accept', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  if (params.all) {
    doc.Revisions.AcceptAll();
  } else if (params.index !== undefined) {
    doc.Revisions.Item(params.index).Accept();
  } else {
    throw new Error('Specify "all": true or "index" parameter');
  }
  return { success: true };
});

Protocol.register('revision.reject', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  if (params.all) {
    doc.Revisions.RejectAll();
  } else if (params.index !== undefined) {
    doc.Revisions.Item(params.index).Reject();
  } else {
    throw new Error('Specify "all": true or "index" parameter');
  }
  return { success: true };
});
