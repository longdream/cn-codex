$ErrorActionPreference = "Stop"
$v = "d:\rustwork\cn-codex\education\goai-preliminary-submission\assets\video"
$s = "d:\rustwork\cn-codex\education\goai-preliminary-submission\assets\screenshots"
$font = "C\:/Windows/Fonts/msyh.ttc"
$vurl = $v -replace '\\','/' -replace ':','\:'

$names = @("01-onboarding","02-plan","03-proactive-modal","04-offline-feedback","05-intervention","06-weekly-review")
$durs  = @(24, 26, 22, 24, 28, 24)

$scenes = @("scene0.mp4")
ffmpeg -y -loop 1 -i "$v\00-title.png" -vf "scale=1920:1080" -t 6 -r 25 -pix_fmt yuv420p -c:v libx264 -preset veryfast "$v\scene0.mp4" | Out-Null

for ($i = 0; $i -lt $names.Count; $i++) {
  $n = $i + 1
  $vf = "crop=w=in_w:h=in_w*9/16:x=0:y=(in_h-in_w*9/16)/2,zoompan=z='min(1+0.06*on/700,1.07)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=700:s=1920x1080:fps=25,drawbox=y=ih-150:w=iw:h=150:color=black@0.55:t=fill,drawtext=fontfile='$font':textfile='$vurl/cap$n.txt':fontsize=36:fontcolor=white:x=(w-text_w)/2:y=h-105"
  ffmpeg -y -loop 1 -i "$s\$($names[$i]).png" -vf $vf -t $durs[$i] -r 25 -pix_fmt yuv420p -c:v libx264 -preset veryfast "$v\scene$n.mp4" | Out-Null
  $scenes += "scene$n.mp4"
}

ffmpeg -y -loop 1 -i "$v\07-ending.png" -vf "scale=1920:1080" -t 8 -r 25 -pix_fmt yuv420p -c:v libx264 -preset veryfast "$v\scene7.mp4" | Out-Null
$scenes += "scene7.mp4"

$list = ($scenes | ForEach-Object { "file '$_'" }) -join "`n"
[System.IO.File]::WriteAllText("$v\list.txt", $list, (New-Object System.Text.UTF8Encoding($false)))
ffmpeg -y -f concat -safe 0 -i "$v\list.txt" -c copy "$v\demo-nosound.mp4" | Out-Null
ffmpeg -y -i "$v\demo-nosound.mp4" -f lavfi -i anullsrc=r=48000:cl=stereo -c:v copy -c:a aac -shortest "$v\demo-3min.mp4" | Out-Null

Write-Output "VIDEO DONE"
